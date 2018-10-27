// fish's Global Inteprreter Lock, the fishgil.
#ifndef FISH_GIL_H
#define FISH_GIL_H
#include "config.h"  // IWYU pragma: keep

#include <condition_variable>
#include <unordered_map>
#include "common.h"

namespace gil_details {
using thread_id_t = uint64_t;

class scheduler_observer_t {
   public:
    virtual ~scheduler_observer_t();
    virtual void did_spawn(thread_id_t tid);
    virtual void will_destroy(thread_id_t tid);
    virtual void did_schedule(thread_id_t tid);
    virtual void will_unschedule(thread_id_t tid);
};

class gil_thread_t;
using gil_thread_ref_t = std::shared_ptr<gil_thread_t>;

class gil_t {
    struct impl_t;
    owning_lock<std::unique_ptr<impl_t>> impl_;
    gil_t();
    gil_t(gil_t &&) = default;

    static std::unique_ptr<gil_t> create_principal_gil();

    /// Schedule the next thread if nothing is scheduled. This must be invoked while the owning lock
    /// for impl is held.
    void schedule_if_needed(impl_t &impl);

   public:
    /// Acquire the run lock. Upon return, the thread will be scheduled.
    void run(gil_thread_ref_t thread);

    /// Release the given thread, which must own the lock. The thread must call run() again to be
    /// rescheduled.
    void release(gil_thread_ref_t thread);

    /// Yield the given thread, which must own the lock. Upon return, the thread reacquires the
    /// lock.
    void yield(gil_thread_ref_t thread);

    static gil_t &gil();
    void add_observer(std::unique_ptr<scheduler_observer_t> var);

    /// \returns true if the given thread is scheduled. This is racey unless called from that
    /// thread.
    bool is_scheduled(gil_thread_ref_t thread);

    ~gil_t();
};

class gil_thread_t {
    friend gil_t;
    const thread_id_t tid;
    std::condition_variable monitor;

   public:
    explicit gil_thread_t();
    virtual ~gil_thread_t();
};

/// variable_t stores a reference to a "thread local" variable, where thread local refers
/// specifically to fish execution threads (not io threads). It manages copies of the variable,
/// keyed by tid.
template <typename T>
class variable_t : public scheduler_observer_t {
    // Map from thread id to value for that thread.
    std::unordered_map<thread_id_t, T> tid_to_vals_;

    /// Address of the published variable.
    T *const published_;

   public:
    explicit variable_t(T *addr) : published_(addr) {}

    void did_spawn(thread_id_t tid) override {
        auto pair = tid_to_vals_.emplace(tid, *published_);
        assert(pair.second && "variable_t should always successfully emplace in did_spawn");
        (void)pair;
    }

    void will_destroy(thread_id_t tid) override {
        auto erased = tid_to_vals_.erase(tid);
        assert(erased == 1 && "variable_t should always have erased");
        (void)erased;
    }

    void will_unschedule(thread_id_t tid) override {
        auto iter = tid_to_vals_.find(tid);
        assert(iter != tid_to_vals_.end() && "can't find olditer in variable_t::will_unschedule");
        std::swap(iter->second, *published_);
    }

    void did_schedule(thread_id_t tid) override {
        auto iter = tid_to_vals_.find(tid);
        assert(iter != tid_to_vals_.end() && "can't find olditer in variable_t::will_schedule");
        std::swap(iter->second, *published_);
    }
};
}  // namespace gil_details

template <typename DATA>
class fish_global_t {
    owning_lock<DATA> data_;

   public:
    fish_global_t() = default;
    fish_global_t(DATA &&d) : data_(std::move(d)) {}
    acquired_lock<DATA> acquire() { return data_.acquire(); }
};

template <typename DATA>
class fish_exec_tld_t {
    using gil_variable_t = gil_details::variable_t<DATA>;
    DATA data_;

    static gil_details::gil_t &gil() { return gil_details::gil_t::gil(); }

public:
    fish_exec_tld_t() { gil().add_observer(make_unique<gil_variable_t>(&data_)); }

    fish_exec_tld_t(DATA &&d) : data_(std::move(d)) {
        gil().add_observer(make_unique<gil_variable_t>(&data_));
    }

    DATA *operator->() { return &data_; }
    const DATA *operator->() const { return &data_; }
    DATA &operator*() { return data_; }
    const DATA &operator*() const { return data_; }
};

#endif
