# Concurrency design

## Background

Unlike other shells, fish only spawns child processes for external commands. fish does not spawn child processes to execute internal fish functions or jobs.

In fish 3.0 and before, when a function is in a pipeline, fish executes the job up until that function (buffering all input), then executes the function in the fish process, then executes the remainder.

fish 3.0 also has a notion of `iothread` which is a POSIX thread which performs IO in the background. This is used for syntax highlighting, etc.

## Problems

The above design is simple, but has the serious problem of needing to buffer all input. For example:

	begin
	    for i in (seq 5)
	      echo $i
	      sleep 1
	    end
	end | grep 1

Here the entirety of the block must execute before grep can run. This creates a perception of slowness or being hung.

## Why this is hard to solve

fish uses the C stack for execution. For example, a for loop is implemented via a synchronous function that executes the body. It is not possible to have two fish functions executing concurrently for the same reason it is not possible to have two C functions executing concurrently (in the same thread).

There are conceptually three ways to solve this:

1. Give each logical process in a pipeline its own child kernel process. `fork()`. This is how other shells solve this. This introduces its own limitations; for example it is not possible for a child process to set a global or environment variable in a parent.
2. Give each process in a pipeline its own kernel thread. This is challenging because of all of the usual multithreading issues, and also issues related to global process state (e.g. the $PWD).
3. Switch fish functions to not use the C stack synchronously, e.g. via async/await, `libuv`, etc. This could work, but is very awkward in C++ because all of the execution logic would have to be inverted. (Maybe C++20 coroutines would make this viable).

fish will use technique #2, and support background execution of fish functions.

## fish 3.1 plans

tl;dr: Python GIL-style threading.

In fish 3.1 we will introduce a new kind of thread, an `exec_thread`. These threads can be used for executing fish script code, including spawning processes.

`iothreads` are normal preemptive threads, but `exec_threads` are cooperatively scheduled. This means they can only execute at specific yield points, and are managed via a user-space scheduler.

The purpose of these yield points is to allow swapping in "instanced state." For example, two concurrently executing fish functions can both use `cd` to change the working directory. The actual working directory will then swap between them as each one is scheduled.

## The Global Interpreter Lock

The fish GIL (ha ha) initially uses a run queue, and a currently executing thread. Each thread that wishes to run is scheduled with the GIL and has an associated condition variable.

Threads will yield at well defined yield points, and then reacquire the GIL. They may perform I/O before reacquiring the GIL, for example reading from a pipe. In this way multiple exec threads can perform I/O but only one can execute fish script.

## Per-Thread vs Global Data

We can choose what data is per-thread and what data is global. For example, if one exec thread creates a function, should other threads be able to execute it?

### Global Data

- Functions
- Completions

### Instanced Data

- `$status`
- `$PWD`

## Design TODOs

- Handle `break` and `continue` in pipelines, presumably by just disallowing them.
- Backtraces

More to come...
