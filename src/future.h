#ifndef FISH_FUTURE_T
#define FISH_FUTURE_T

#include <cassert>
#include <functional>
#include "maybe.h"

template<typename T>
class future_t {
    future_t(const future_t &) = delete;
    void operator=(const future_t &) = delete;
    maybe_t<T> value_;
public:
    future_t(future_t &&) = default;
    future_t &operator=(future_t &&) = default;

    /* implicit */ future_t(T val) : value_(std::move(val)) {}
    future_t() {}

    const T &value() const {
        assert(value_ && "Value not ready");
        return *value_;
    }

    T acquire() {
        assert(value_ && "Value not ready");
        return value_.acquire();
    }

    template <typename Func>
    typename std::result_of<Func(T)>::type then(Func func) {
        return func(acquire());
    }
} __warn_unused;

#endif

