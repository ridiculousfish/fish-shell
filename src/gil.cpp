#include "gil.h"

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

    void will_schedule(thread_id_t oldtid, thread_id_t newtid) override {
        auto iter = tid_to_pwd_.find(newtid);
        assert(iter != tid_to_pwd_.end() && "newtid not found in cd_observer_t");
        int err = chdir(iter->second.c_str());
        (void)err;
    }
};
}  // namespace

struct gil_t::impl_t {
    std::vector<std::unique_ptr<scheduler_observer_t>> observers;
};

gil_t::gil_t() : impl_(make_unique<impl_t>()) {}
gil_t::~gil_t() = default;

gil_t gil_t::create_principal_gil() {
    gil_t result;
    result.add_observer(make_unique<cd_observer_t>());
    return result;
}

gil_t &gil_details::gil_t::gil() {
    static gil_t gil = create_principal_gil();
    return gil;
}

void gil_details::gil_t::add_observer(std::unique_ptr<scheduler_observer_t> obs) {
    impl_->observers.push_back(std::move(obs));
}

void gil_details::scheduler_observer_t::did_spawn(thread_id_t tid) {}
void gil_details::scheduler_observer_t::will_destroy(thread_id_t tid) {}
void gil_details::scheduler_observer_t::will_schedule(thread_id_t oldtid, thread_id_t newtid) {}

gil_details::scheduler_observer_t::~scheduler_observer_t() = default;
