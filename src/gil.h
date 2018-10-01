// fish's Global Inteprreter Lock, the fishgil.
#ifndef FISH_GIL_H
#define FISH_GIL_H
#include "config.h"  // IWYU pragma: keep

#include <unordered_map>
#include "common.h"

namespace gil_details {
using thread_id_t = uint64_t;

class scheduler_observer_t {
   public:
    virtual ~scheduler_observer_t();
    virtual void did_spawn(thread_id_t tid);
    virtual void will_destroy(thread_id_t tid);
    virtual void will_schedule(thread_id_t oldtid, thread_id_t newtid);
};

class gil_t {
    struct impl_t;
    std::unique_ptr<impl_t> impl_;
    gil_t();
    ~gil_t();
    gil_t(gil_t &&) = default;

    static gil_t create_principal_gil();

   public:
    static gil_t &gil();
    void add_observer(std::unique_ptr<scheduler_observer_t> var);
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

    void will_schedule(thread_id_t oldtid, thread_id_t newtid) override {
        auto olditer = tid_to_vals_.find(oldtid);
        auto newiter = tid_to_vals_.find(newtid);
        assert(olditer != tid_to_vals_.end() && "can't find olditer in variable_t::will_schedule");
        assert(olditer != tid_to_vals_.end() && "can't find newiter in variable_t::will_schedule");
        olditer->second = std::move(*published_);
        *published_ = std::move(newiter->second);
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
