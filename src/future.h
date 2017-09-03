#ifndef FISH_FUTURE_T
#define FISH_FUTURE_T

#include <cassert>
#include <functional>
#include <memory>
#include "maybe.h"

template <class T>
using result_of_t = typename std::result_of<T>::type;

template<typename T>
class future_t {
    using callback_t = std::function<void(T)>;

    struct guts_t {
        maybe_t<T> value_;
        callback_t callback_;
        bool fulfilled_ = false;
        bool callbackInvoked_ = false;

        void fulfill(T value) {
            assert(!fulfilled_ && "Future already fulfilled");
            fulfilled_ = true;
            value_ = std::move(value);
            maybeInvokeCallback();
        }

        void set_callback(callback_t callback) {
            assert(!callback_ && !callbackInvoked_ && "Callback already set");
            callback_ = std::move(callback);
            maybeInvokeCallback();
        }

        void maybeInvokeCallback() {
            if (fulfilled_ && !callbackInvoked_ && callback_) {
                callbackInvoked_ = true;
                callback_(value_.acquire());
                callback_ = callback_t{};
            }
        }
    };
    std::shared_ptr<guts_t> guts_;

    future_t(const future_t &) = delete;
    void operator=(const future_t &) = delete;

    future_t(std::shared_ptr<guts_t> guts) : guts_(std::move(guts)) {}

    template <typename Func>
    static void iterate_helper(Func func, std::function<void(const T&)> fulfiller) {
        func().on_complete([=](const maybe_t<T> &result) {
            if (result.has_value()) {
                fulfiller(*result);
            } else {
                iterate_helper(func, fulfiller);
            }
        });
    }

   public:
    future_t() {}
    future_t(future_t &&) = default;
    future_t &operator=(future_t &&) = default;

    /* implicit */ future_t(T val) : guts_(std::make_shared<guts_t>()) {
        guts_->fulfill(std::move(val));
    }

    T acquire() {
        assert(guts_->value_ && "Value not ready");
        return guts_->value_.acquire();
    };

    template <typename Func>
    result_of_t<Func(T)> then(Func func) {
        assert(guts_ && "future is uninstantiated");
        using next_future_t = result_of_t<Func(T)>;
        auto future_fulfiller = next_future_t::create();
        auto fulfiller = std::move(future_fulfiller.second);
        this->guts_->set_callback([=](T value){
            auto new_future = func(std::move(value));
            new_future.guts_->set_callback(fulfiller);
        });
        return std::move(future_fulfiller.first);
    }

    future_t on_complete(std::function<void(const T &)> func) {
        return then([func](T val) -> future_t<T> {
            func(val);
            return future_t<T>(std::move(val));
        });
    }

    future_t on_complete(std::function<void(void)> func) {
        return on_complete([func](const T &) { func(); });
    }

    template <typename Func>
    future_t<result_of_t<Func(T)>> map(Func func) {
        auto future_fulfiller = future_t<result_of_t<Func(T)>>::create();
        auto fulfiller = std::move(future_fulfiller.second);
        this->guts_->set_callback([=](T value){
            fulfiller(func(value));
        });
        return std::move(future_fulfiller.first);
    }

    // Let F be a function void->future_t<maybe_t<T>>. Iterate the function and await the result
    // while it returns none. Returns the resulting value from the maybe.
    static future_t<T> iterate(std::function<future_t<maybe_t<T>>()> func) {
        auto future_fulfiller = future_t<T>::create();
        iterate_helper(std::move(func), std::move(future_fulfiller.second));
        return std::move(future_fulfiller.first);
    };

    using fulfiller_t = std::function<void(T)>;

    const T &value() const {
        assert(guts_ && "future is uninstantiated");
        // Temporary hack.
        return *guts_->value_;
    }

    static __warn_unused std::pair<future_t, fulfiller_t> create() {
        auto guts = std::make_shared<guts_t>();
        std::weak_ptr<guts_t> weak_guts(guts);
        fulfiller_t fulfiller = [weak_guts](T val){
            if (auto guts = weak_guts.lock()) {
                guts->fulfill(std::move(val));
            }
        };
        return {future_t(std::move(guts)), std::move(fulfiller)};
    }
} __warn_unused;

#endif

