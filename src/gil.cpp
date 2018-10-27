#include "gil.h"

#include <atomic>
#include <condition_variable>
#include <deque>

#include <sys/param.h>

using namespace gil_details;

namespace {
class cd_observer_t : public scheduler_observer_t {
    std::unordered_map<thread_id_t, std::string> tid_to_pwd_;

    static std::string get_current_directory() {
        char buff[PATH_MAX];
        if (!getcwd(buff, sizeof buff)) {
            return "/";
        }
        return std::string{buff};
    }

    void did_spawn(thread_id_t tid) override { tid_to_pwd_[tid] = get_current_directory(); }

    void will_destroy(thread_id_t tid) override { tid_to_pwd_.erase(tid); }

    void will_unschedule(thread_id_t tid) override {
        // Save the cwd.
        auto iter = tid_to_pwd_.find(tid);
        assert(iter != tid_to_pwd_.end() && "tid not found in cd_observer_t");
        iter->second = get_current_directory();
    }

    void did_schedule(thread_id_t tid) override {
        auto iter = tid_to_pwd_.find(tid);
        assert(iter != tid_to_pwd_.end() && "tid not found in cd_observer_t");
        int err = chdir(iter->second.c_str());
        (void)err;
    }
};
}  // namespace

struct gil_t::impl_t {
    /// Scheduling observers.
    std::deque<std::unique_ptr<scheduler_observer_t>> observers;

    /// List of threads blocked in run(), waiting to be scheduled.
    std::deque<gil_thread_ref_t> waitqueue;

    /// The currently running thread.
    gil_thread_ref_t owner;
};

gil_t::gil_t() : impl_(make_unique<impl_t>()) {}
gil_t::~gil_t() = default;

std::unique_ptr<gil_t> gil_t::create_principal_gil() {
    std::unique_ptr<gil_t> result{new gil_t()};
    result->add_observer(make_unique<cd_observer_t>());
    return result;
}

gil_t &gil_details::gil_t::gil() {
    static std::unique_ptr<gil_t> gil = create_principal_gil();
    return *gil;
}

bool gil_t::is_scheduled(gil_thread_ref_t thread) {
    auto lock = impl_.acquire();
    auto &impl = *lock;
    return thread && thread == impl->owner;
}

void gil_t::run(gil_thread_ref_t thread) {
    assert(thread && "null thread in gil_t::run");
    auto lock = impl_.acquire();
    auto &impl = *lock;

    // Put ourselves onto the waitqueue and wait until we are scheduled.
    impl->waitqueue.push_back(thread);
    schedule_if_needed(*impl);
    while (impl->owner != thread) {
        thread->monitor.wait(lock.get_ulock());
    }
    // Note that we are now scheduled.
    for (auto &obs : impl->observers) {
        obs->did_schedule(thread->tid);
    }
}

void gil_t::yield(gil_thread_ref_t thread) {
    release(thread);
    run(thread);
}

void gil_t::release(gil_thread_ref_t old_thread) {
    auto lock = impl_.acquire();
    auto &impl = *lock;
    assert(old_thread == impl->owner && "Old thread was not running");
}

void gil_t::schedule_if_needed(impl_t &impl) {
    // Do nothing if we're already scheduled, or if we have nothing to schedule.
    if (impl.owner || impl.waitqueue.empty()) return;
    impl.owner = std::move(impl.waitqueue.front());
    impl.waitqueue.pop_front();
    impl.owner->monitor.notify_one();
}

void gil_details::gil_t::add_observer(std::unique_ptr<scheduler_observer_t> obs) {
    auto lock = impl_.acquire();
    auto &impl = *lock;
    impl->observers.push_back(std::move(obs));
}

void gil_details::scheduler_observer_t::did_spawn(thread_id_t tid) {}
void gil_details::scheduler_observer_t::will_destroy(thread_id_t tid) {}
void gil_details::scheduler_observer_t::did_schedule(thread_id_t tid) {}
void gil_details::scheduler_observer_t::will_unschedule(thread_id_t tid) {}

gil_details::scheduler_observer_t::~scheduler_observer_t() = default;

static std::atomic<uint64_t> s_last_tid;
gil_thread_t::gil_thread_t() : tid(++s_last_tid) {}

gil_thread_t::~gil_thread_t() = default;
