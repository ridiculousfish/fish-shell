# Concurrent Execution

This describes the plan for subshells and fully parallel pipelines in fish-shell.

## Background

"Subshell" refers to creating a separately-executing copy of the shell with its own state, such as its own `$PWD`, variables, etc.

"Fully parallel pipelines" means that two or more fish functions or builtins can execute at once.

"Background functions" means that a shell function can run in the background, as external commands can today.

These are typically related in other shells through through `fork()`. Forking allows creating a new shell inheriting state from its parent. It also allows builtins and functions to each get their own process so they can execute in parallel.

For better or for worse, fish cannot adopt the `fork()` concurrency model. Even if the fork-thread interactions were to be resolved (possible though difficult), it would change fish semantics so that variables set in pipelines would no longer work. A silly example is `begin; set -g foo bar; end | cat`; this would no longer set a global variable because the block would run in a separate process. This would break too much, so fish will instead implement these features differently.

## High level plan

1. Subshells will be implemented by tracking shell state internally, rather than relying on process-level isolation. That is, it will be possible for two or more fish functions to be running at the same time, with two sets of variables and in particular two different `$PWD`.

2. Parallel pipelines will be implemented either via running functions in different threads, or through Rust `async` (TBD).

3. Background functions will use the machinery introduced for parallel pipelines to run a function in parallel with the interactive reader.

A key design goal is that this is turned on for everyone, not something you have to opt into. This means some changing of semantics (discussed below) and some unavoidable risk of breaking existing scripts; but the benefits are worth it.

## Implementation

### Parser and branch

fish internally has a type `Parser` which is "a thing that can run fish script." A Parser also has its own set of variables and tracks its own `$PWD`.

We will teach Parser how to `branch()`: birth a new Parser that inherits initial state from its parent. This is the moral equivalent of `fork`. Subshells will be implemented by branching the parent Parser and running the subshell command in this child.

## Local and global state

It is important to be precise about what state is local to the subshell, and what state is global. (This is a nice aspect of our design: with `fork` that decision is determined by the kernel.)

To say that state is "global" means that if it is modified in a subshell, then the parent and any other subshell sees the change instantly. To say it is "local" means that two different subshells can disagree on the value.

### Global state

- Functions
- Key bindings
- Completions
- Event handlers
- Job control mode (set with `status`, rarely used)
- Global-scoped and universal variables
- Local or function-scoped variables _from the parent of the subshell_

### Local state

- `cd` and `$PWD`
- `$status` and `$pipestatus`
- Variables scoped to be inside the subshell (e.g. with `-l`)
- Backtraces
- `status` commands which inspect the backtrace (e.g. `status is-block`)

#### Parent variables

To make the "parent variables" idea concrete: imagine accumulating the counts of files in each directory. Here `begin --subshell` is illustrative syntax only.

```shell
set counts
for dir in */
    begin --subshell
        cd $dir
        set --append counts (count *.txt)
    end
end
```

Now `$counts` is a list of the number of text files in each directory; this would be impossible if subshells were implemented using `fork`. Also note that `cd` to return to the parent is NOT required.
