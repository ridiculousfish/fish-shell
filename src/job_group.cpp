#include "config.h"

#include "job_group.h"

#include "common.h"
#include "fallback.h"  // IWYU pragma: keep
#include "flog.h"
#include "future_feature_flags.h"
#include "postfork.h"
#include "proc.h"
#include "wutil.h"

// Basic thread safe sorted vector of job IDs in use.
// This is deliberately leaked to avoid dtor ordering issues - see #6539.
static const auto locked_consumed_job_ids = new owning_lock<std::vector<job_id_t>>();

static job_id_t acquire_job_id() {
    auto consumed_job_ids = locked_consumed_job_ids->acquire();

    // The new job ID should be larger than the largest currently used ID (#6053).
    job_id_t jid = consumed_job_ids->empty() ? 1 : consumed_job_ids->back() + 1;
    consumed_job_ids->push_back(jid);
    return jid;
}

static void release_job_id(job_id_t jid) {
    assert(jid > 0);
    auto consumed_job_ids = locked_consumed_job_ids->acquire();

    // Our job ID vector is sorted, but the number of jobs is typically 1 or 2 so a binary search
    // isn't worth it.
    auto where = std::find(consumed_job_ids->begin(), consumed_job_ids->end(), jid);
    assert(where != consumed_job_ids->end() && "Job ID was not in use");
    consumed_job_ids->erase(where);
}

job_group_t::~job_group_t() {
    if (owns_pgid_) {
        // We own the pgid; waitpid() on it.
        int stat = -1;
        if (waitpid(*pgid_, &stat, 0) < 0) {
            wperror(L"waitpid");
        }
    }
    if (props_.job_id > 0) {
        release_job_id(props_.job_id);
    }
}

void job_group_t::set_pgid(pid_t pgid) {
    // Note we need not be concerned about thread safety. job_groups are intended to be shared
    // across threads, but their pgid should always have been set beforehand.
    assert(needs_pgid_assignment() && "We should not be setting a pgid");
    assert(pgid >= 0 && "Invalid pgid");
    pgid_ = pgid;
}

maybe_t<pid_t> job_group_t::get_pgid() const { return pgid_; }

/// \return a new pid which can serve as a pgroup owner.
/// The child process exits immediately.
static pid_t create_owned_pgid(const wcstring &cmd) {
    pid_t pid = execute_fork();
    assert(pid >= 0 && "execute_fork should never return an invalid pid");
    if (pid == 0) {
        // The child can just exit directly; all we need is a pid which we can defer reaping.
        exit_without_destructors(0);
        DIE("exit_without_destructors should not return");
    }
    if (setpgid(pid, pid)) {
        wperror(L"setpgid");
    }
    FLOG(exec_fork, "Fork", pid, "to act as pgroup owner for", cmd);
    return pid;
}

void job_group_t::populate_group_for_job(job_t *job, const job_group_ref_t &proposed) {
    assert(!job->group && "Job already has a group");
    // Note there's three cases to consider:
    //  nullptr         -> this is a root job, there is no inherited job group
    //  internal        -> the parent is running as part of a simple function execution
    //                      We may need to create a new job group if we are going to fork.
    //  non-internal    -> we are running as part of a real pipeline
    // Decide if this job can use an internal group.
    // This is true if it's a simple foreground execution of an internal proc.
    bool initial_bg = job->is_initially_background();
    bool first_proc_internal = job->processes.front()->is_internal();
    bool can_use_internal =
        !initial_bg && job->processes.size() == 1 && job->processes.front()->is_internal();

    bool needs_new_group = false;
    if (!proposed) {
        // We don't have a group yet.
        needs_new_group = true;
    } else if (initial_bg) {
        // Background jobs always get a new group.
        needs_new_group = true;
    } else if (proposed->is_internal() && !can_use_internal) {
        // We cannot use the internal group for this job.
        needs_new_group = true;
    }

    job->mut_flags().is_group_root = needs_new_group;

    if (!needs_new_group) {
        job->group = proposed;
    } else {
        properties_t props{};
        props.job_control = job->wants_job_control();
        props.wants_terminal = job->wants_job_control() && !job->from_event_handler();
        props.is_internal = can_use_internal;
        props.job_id = can_use_internal ? -1 : acquire_job_id();
        job_group_ref_t group = job_group_ref_t(new job_group_t(props, job->command()));

        // Mark if it's foreground.
        group->set_is_foreground(!initial_bg);

        // Perhaps this job should immediately live in fish's pgroup.
        // There's two reasons why it may be so:
        //  1. The job doesn't need job control.
        //  2. The first process in the job is internal to fish; this needs to own the tty.
        if (!can_use_internal && (!props.job_control || first_proc_internal)) {
            group->set_pgid(getpgrp());
        }

        // Perhaps we should fork a process for this job immediately.
        // This happens if concurrent execution is enabled, and our job contains at least one
        // internal process. It's important that all processes end up in the same process group
        // so that signal delivery works.
        // TODO: in principle this could be deferred until it is needed. Certain pipelines may never
        // even need a pgroup.
        if (feature_test(features_t::concurrent) && !group->get_pgid() &&
            job->processes.size() > 1 && job->has_internal_proc()) {
            group->set_pgid(create_owned_pgid(job->command()));
            group->owns_pgid_ = true;
        }

        job->group = std::move(group);
    }
}
