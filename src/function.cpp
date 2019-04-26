// Functions for storing and retrieving function information. These functions also take care of
// autoloading functions in the $fish_function_path. Actual function evaluation is taken care of by
// the parser and to some degree the builtin handling library.
//
#include "config.h"  // IWYU pragma: keep

// IWYU pragma: no_include <type_traits>
#include <dirent.h>
#include <pthread.h>
#include <stddef.h>
#include <cwchar>

#include <algorithm>
#include <map>
#include <memory>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>

#include "autoload.h"
#include "common.h"
#include "env.h"
#include "event.h"
#include "exec.h"
#include "fallback.h"  // IWYU pragma: keep
#include "function.h"
#include "intern.h"
#include "parser.h"
#include "parser_keywords.h"
#include "reader.h"
#include "wutil.h"  // IWYU pragma: keep

class function_info_t {
   public:
    /// Immutable properties of the function.
    std::shared_ptr<const function_properties_t> props;
    /// Function description. This may be changed after the function is created.
    wcstring description;
    /// File where this function was defined (intern'd string).
    const wchar_t *const definition_file;
    /// Mapping of all variables that were inherited from the function definition scope to their
    /// values.
    const std::map<wcstring, env_var_t> inherit_vars;
    /// Flag for specifying that this function was automatically loaded.
    const bool is_autoload;

    /// Constructs relevant information from the function_data.
    function_info_t(function_data_t data, const environment_t &vars, const wchar_t *filename,
                    bool autoload);

    /// Used by function_copy.
    function_info_t(const function_info_t &data, const wchar_t *filename, bool autoload);
};

/// Type wrapping up the set of all functions.
/// There's only one of these; it's managed by a lock.
struct function_set_t {
    /// The map of all functions by name.
    std::unordered_map<wcstring, function_info_t> funcs_;

    /// Tombstones for functions that should no longer be autoloaded.
    std::unordered_set<wcstring> autoload_tombstones_;

    /// The autoloader for our functions.
    /// This is never null.
    std::unique_ptr<autoload_observer_t> autoloader_;

    /// A map from command to autoloadable files.
    /// This is used to detect when an autoloadable function changes.
    std::unordered_map<wcstring, autoloadable_file_t> autoloadable_files_;

    /// The set of function names that are currently being autoloaded.
    std::unordered_set<wcstring> current_autoloading_;

    /// Remove a function.
    /// \return true if successful, false if it doesn't exist.
    bool remove(const wcstring &name);

    /// Get the info for a function, or nullptr if none.
    const function_info_t *get_info(const wcstring &name) const {
        auto iter = funcs_.find(name);
        return iter == funcs_.end() ? nullptr : &iter->second;
    }

    /// \return a file to autoload, given a name.
    maybe_t<autoloadable_file_t> should_autoload(const wcstring &name) const;

    /// If our autoload paths (i.e. fish_function_path) has changed, update it and create a new
    /// observer.
    void update_autoload_paths();

    function_set_t() : autoloader_(make_unique<autoload_observer_t>(wcstring_list_t())) {}
};

/// The big set of all functions.
static owning_lock<function_set_t> function_set;

void function_set_t::update_autoload_paths() {
    const wcstring_list_t empty;
    const auto &vars = parser_t::principal_parser().vars();
    auto var = vars.get(L"fish_function_path");
    const wcstring_list_t &expected_dirs = var ? var->as_list() : empty;
    if (autoloader_->dirs() != expected_dirs) {
        // Throw away the autoloader and make a new one with the new paths.
        // Note we don't have to update any of our internal state.
        autoloader_ = make_unique<autoload_observer_t>(expected_dirs);
    }
}

/// \return a file that we should autoload given a function name, or none() if none.
maybe_t<autoloadable_file_t> function_set_t::should_autoload(const wcstring &name) const {
    // Do we already have a real function?
    const auto *info = get_info(name);
    if (info && !info->is_autoload) {
        return none();
    }

    // Is this function tombstoned?
    if (autoload_tombstones_.count(name) > 0) {
        return none();
    }

    // Are we currently in the process of autoloading this?
    if (current_autoloading_.count(name) > 0) {
        return none();
    }

    // Ask our autoloader what to do.
    // If it doesn't have a file, there's nothing to do.
    auto mfile = autoloader_->check(name);
    if (!mfile) {
        return none();
    }

    // Is this file the same as what we previously autoloaded?
    auto iter = autoloadable_files_.find(name);
    if (iter != autoloadable_files_.end()) {
        const autoloadable_file_t &current = iter->second;
        if (current.file_id == mfile->file_id && current.path == mfile->path) {
            // The file is unchanged.
            return none();
        }
    }

    return mfile;
}

/// Perform autoload on a given path, with the given parser.
static void do_autoload_file_at_path(const wcstring &path) {
    wcstring script_source = L"source " + escape_string(path, ESCAPE_ALL);
    exec_subshell(script_source, parser_t::principal_parser(),
                  false /* do not apply exit status */);
}

/// Make sure that if the specified function is a dynamically loaded function, it has been fully
/// loaded.
/// Note this executes fish script code.
static void try_autoload(const wcstring &name) {
    ASSERT_IS_MAIN_THREAD();
    wcstring path_to_autoload;

    // Note we can't autoload while holding the funcset lock.
    // Lock around a local region.
    {
        auto funcset = function_set.acquire();

        // Take this opportunity to update (or perhaps initialize) our autoload paths.
        funcset->update_autoload_paths();

        maybe_t<autoloadable_file_t> mfile = funcset->should_autoload(name);
        if (mfile && !mfile->path.empty()) {
            funcset->current_autoloading_.insert(name);
            funcset->autoloadable_files_[name] = *mfile;
            path_to_autoload = std::move(mfile->path);
        }
    }

    // Release the lock and perform any autoload, the reacquire the lock and clean up.
    if (!path_to_autoload.empty()) {
        // Crucially, the lock is acquired *after* do_autoload_file_at_path().
        do_autoload_file_at_path(path_to_autoload);
        auto funcset = function_set.acquire();
        funcset->current_autoloading_.erase(name);
    }
}

/// Insert a list of all dynamically loaded functions into the specified list.
static void autoload_names(std::unordered_set<wcstring> &names, int get_hidden) {
    size_t i;

    // TODO: justfy this.
    auto &vars = env_stack_t::principal();
    const auto path_var = vars.get(L"fish_function_path");
    if (path_var.missing_or_empty()) return;

    wcstring_list_t path_list;
    path_var->to_list(path_list);

    for (i = 0; i < path_list.size(); i++) {
        const wcstring &ndir_str = path_list.at(i);
        dir_t dir(ndir_str);
        if (!dir.valid()) continue;

        wcstring name;
        while (dir.read(name)) {
            const wchar_t *fn = name.c_str();
            const wchar_t *suffix;
            if (!get_hidden && fn[0] == L'_') continue;

            suffix = std::wcsrchr(fn, L'.');
            if (suffix && (std::wcscmp(suffix, L".fish") == 0)) {
                wcstring name(fn, suffix - fn);
                names.insert(name);
            }
        }
    }
}

static std::map<wcstring, env_var_t> snapshot_vars(const wcstring_list_t &vars,
                                                   const environment_t &src) {
    std::map<wcstring, env_var_t> result;
    for (const wcstring &name : vars) {
        auto var = src.get(name);
        if (var) result[name] = std::move(*var);
    }
    return result;
}

function_info_t::function_info_t(function_data_t data, const environment_t &vars,
                                 const wchar_t *filename, bool autoload)
    : props(std::make_shared<const function_properties_t>(std::move(data.props))),
      description(std::move(data.description)),
      definition_file(intern(filename)),
      inherit_vars(snapshot_vars(data.inherit_vars, vars)),
      is_autoload(autoload) {}

function_info_t::function_info_t(const function_info_t &data, const wchar_t *filename,
                                 bool autoload)
    : props(data.props),
      description(data.description),
      definition_file(intern(filename)),
      inherit_vars(data.inherit_vars),
      is_autoload(autoload) {}

void function_add(const function_data_t &data, const parser_t &parser) {
    ASSERT_IS_MAIN_THREAD();
    auto funcset = function_set.acquire();

    // Historical check. TODO: rationalize this.
    if (data.name.empty()) {
        return;
    }

    // Remove the old function.
    funcset->remove(data.name);

    // Check if this is a function that we are autoloading.
    bool is_autoload = funcset->current_autoloading_.count(data.name) > 0;

    // Create and store a new function.
    const wchar_t *filename = reader_current_filename();
    auto ins = funcset->funcs_.emplace(data.name,
                                       function_info_t(data, parser.vars(), filename, is_autoload));
    assert(ins.second && "Function should not already be present in the table");
    (void)ins;

    // Add event handlers.
    for (const event_description_t &ed : data.events) {
        event_add_handler(std::make_shared<event_handler_t>(ed, data.name));
    }
}

std::shared_ptr<const function_properties_t> function_get_properties(const wcstring &name) {
    if (parser_keywords_is_reserved(name)) return nullptr;
    auto funcset = function_set.acquire();
    if (const auto *info = funcset->get_info(name)) {
        return info->props;
    }
    return nullptr;
}

int function_exists(const wcstring &cmd) {
    ASSERT_IS_MAIN_THREAD();
    if (parser_keywords_is_reserved(cmd)) return 0;
    try_autoload(cmd);
    auto funcset = function_set.acquire();
    return funcset->funcs_.find(cmd) != funcset->funcs_.end();
}

void function_load(const wcstring &cmd) {
    ASSERT_IS_MAIN_THREAD();
    if (!parser_keywords_is_reserved(cmd)) {
        try_autoload(cmd);
    }
}

int function_exists_no_autoload(const wcstring &cmd, const environment_t &vars) {
    if (parser_keywords_is_reserved(cmd)) return 0;
    auto funcset = function_set.acquire();

    // Do we actually have this function?
    if (funcset->funcs_.find(cmd) != funcset->funcs_.end()) {
        return true;
    }

    // Could it conceivably be autoloaded?
    // We permit stale accesses here since we don't plan to load it.
    bool allow_stale = true;
    return funcset->autoloader_->check(cmd, allow_stale).has_value();
}

bool function_set_t::remove(const wcstring &name) {
    auto iter = funcs_.find(name);
    if (iter == funcs_.end()) {
        return false;
    }

    // Forget it from our autoloadable.
    // Note we don't tombstone it here, since this is called from function_add(). We only want to
    // prohibit autoloading if the user explicitly removes a function.
    autoloadable_files_.erase(name);

    // Remove any handlers.
    event_remove_function_handlers(name);

    // Remove the function itself.
    funcs_.erase(iter);
    return true;
}

void function_remove(const wcstring &name) {
    auto funcset = function_set.acquire();
    if (funcset->remove(name)) {
        // Prevent re-autoloading this function.
        funcset->autoload_tombstones_.insert(name);
    }
}

bool function_get_definition(const wcstring &name, wcstring &out_definition) {
    const auto funcset = function_set.acquire();
    if (const function_info_t *func = funcset->get_info(name)) {
        auto props = func->props;
        if (props && props->parsed_source) {
            out_definition = props->body_node.get_source(props->parsed_source->src);
        }
        return true;
    }
    return false;
}

std::map<wcstring, env_var_t> function_get_inherit_vars(const wcstring &name) {
    const auto funcset = function_set.acquire();
    const function_info_t *func = funcset->get_info(name);
    return func ? func->inherit_vars : std::map<wcstring, env_var_t>();
}

bool function_get_desc(const wcstring &name, wcstring &out_desc) {
    const auto funcset = function_set.acquire();
    const function_info_t *func = funcset->get_info(name);
    if (func && !func->description.empty()) {
        out_desc = _(func->description.c_str());
        return true;
    }
    return false;
}

void function_set_desc(const wcstring &name, const wcstring &desc) {
    ASSERT_IS_MAIN_THREAD();
    try_autoload(name);
    auto funcset = function_set.acquire();
    auto iter = funcset->funcs_.find(name);
    if (iter != funcset->funcs_.end()) {
        iter->second.description = desc;
    }
}

bool function_copy(const wcstring &name, const wcstring &new_name) {
    auto funcset = function_set.acquire();
    auto iter = funcset->funcs_.find(name);
    if (iter == funcset->funcs_.end()) {
        // No such function.
        return false;
    }

    // This new instance of the function shouldn't be tied to the definition file of the
    // original, so pass NULL filename, etc.
    // Note this will NOT overwrite an existing function with the new name. TODO: rationalize if
    // that is desired.
    funcset->funcs_.emplace(new_name, function_info_t(iter->second, nullptr, false));
    return true;
}

wcstring_list_t function_get_names(int get_hidden) {
    std::unordered_set<wcstring> names;
    auto funcset = function_set.acquire();
    autoload_names(names, get_hidden);
    for (const auto &func : funcset->funcs_) {
        const wcstring &name = func.first;

        // Maybe skip hidden.
        if (!get_hidden && (name.empty() || name.at(0) == L'_')) {
            continue;
        }
        names.insert(name);
    }
    return wcstring_list_t(names.begin(), names.end());
}

const wchar_t *function_get_definition_file(const wcstring &name) {
    const auto funcset = function_set.acquire();
    const function_info_t *func = funcset->get_info(name);
    return func ? func->definition_file : NULL;
}

bool function_is_autoloaded(const wcstring &name) {
    const auto funcset = function_set.acquire();
    const function_info_t *func = funcset->get_info(name);
    return func ? func->is_autoload : false;
}

int function_get_definition_lineno(const wcstring &name) {
    const auto funcset = function_set.acquire();
    const function_info_t *func = funcset->get_info(name);
    if (!func) return -1;
    // return one plus the number of newlines at offsets less than the start of our function's
    // statement (which includes the header).
    // TODO: merge with line_offset_of_character_at_offset?
    auto block_stat = func->props->body_node.try_get_parent<grammar::block_statement>();
    assert(block_stat && "Function body is not part of block statement");
    auto source_range = block_stat.source_range();
    assert(source_range && "Function has no source range");
    uint32_t func_start = source_range->start;
    const wcstring &source = func->props->parsed_source->src;
    assert(func_start <= source.size() && "function start out of bounds");
    return 1 + std::count(source.begin(), source.begin() + func_start, L'\n');
}

void function_invalidate_path() {
    // Remove all autoloaded functions and update the autoload path.
    // Note we don't want to risk removal during iteration; we expect this to be called
    // infrequently.
    auto funcset = function_set.acquire();
    wcstring_list_t autoloadees;
    for (const auto &kv : funcset->funcs_) {
        if (kv.second.is_autoload) {
            autoloadees.push_back(kv.first);
        }
    }
    for (const wcstring &name : autoloadees) {
        funcset->remove(name);
    }
    funcset->autoloadable_files_.clear();
}

// Setup the environment for the function. There are three components of the environment:
// 1. argv
// 2. named arguments
// 3. inherited variables
void function_prepare_environment(env_stack_t &vars, const wcstring &name,
                                  const wchar_t *const *argv,
                                  const std::map<wcstring, env_var_t> &inherited_vars) {
    vars.set_argv(argv);
    auto props = function_get_properties(name);
    if (props && !props->named_arguments.empty()) {
        const wchar_t *const *arg = argv;
        for (const wcstring &named_arg : props->named_arguments) {
            if (*arg) {
                vars.set_one(named_arg, ENV_LOCAL | ENV_USER, *arg);
                arg++;
            } else {
                vars.set_empty(named_arg, ENV_LOCAL | ENV_USER);
            }
        }
    }

    for (const auto &kv : inherited_vars) {
        vars.set(kv.first, ENV_LOCAL | ENV_USER, kv.second.as_list());
    }
}
