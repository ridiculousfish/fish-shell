.. _configuration:

Configuration files
====================

When fish is started, it reads and runs its configuration files. Where these are depends on build configuration and environment variables.

The main file is ``~/.config/fish/config.fish`` (or more precisely ``$XDG_CONFIG_HOME/fish/config.fish``).

Configuration files are run in the following order:

- Configuration snippets (named ``*.fish``) in the directories:

  - ``$__fish_config_dir/conf.d`` (by default, ``~/.config/fish/conf.d/``)
  - ``$__fish_sysconf_dir/conf.d`` (by default, ``/etc/fish/conf.d/``)
  - Directories for others to ship configuration snippets for their software. Fish searches the directories in the ``XDG_DATA_DIRS`` environment variable for a ``fish/vendor_conf.d`` directory; if that is not defined, the default is ``/usr/share/fish/vendor_conf.d`` and ``/usr/local/share/fish/vendor_conf.d``, unless your distribution customized this.

  If there are multiple files with the same name in these directories, only the first will be executed.
  They are executed in order of their filename, sorted (like globs) in a natural order (i.e. "01" sorts before "2").

- System-wide configuration files, where administrators can include initialization for all users on the system - similar to ``/etc/profile`` for POSIX-style shells - in ``$__fish_sysconf_dir`` (usually ``/etc/fish/config.fish``).
- The user's auto-config file, usually in ``~/.config/fish/config.auto.fish`` (controlled by the ``XDG_CONFIG_HOME`` environment variable, and accessible as ``$__fish_config_dir``).
- User configuration, usually in ``~/.config/fish/config.fish`` (controlled by the same variables as above).

``~/.config/fish/config.fish`` is sourced *after* the snippets. This is so you can copy snippets and override some of their behavior.

These files are all executed on the startup of every shell. If you want to run a command only on starting an interactive shell, use the exit status of the command ``status --is-interactive`` to determine if the shell is interactive. If you want to run a command only when using a login shell, use ``status --is-login`` instead. This will speed up the starting of non-interactive or non-login shells.

If you are developing another program, you may want to add configuration for all users of fish on a system. This is discouraged; if not carefully written, they may have side-effects or slow the startup of the shell. Additionally, users of other shells won't benefit from the fish-specific configuration. However, if they are required, you can install them to the "vendor" configuration directory. As this path may vary from system to system, ``pkg-config`` should be used to discover it: ``pkg-config --variable confdir fish``.

Examples:

If you want to add the directory ``~/linux/bin`` to your PATH variable when using a login shell, add this to your ``~/.config/fish/config.fish`` file::

    if status --is-login
        set -gx PATH $PATH ~/linux/bin
    end

(alternatively use :ref:`fish_add_path <cmd-fish_add_path>` like ``fish_add_path ~/linux/bin``, which only adds the path if it isn't included yet)

If you want to run a set of commands when fish exits, use an :ref:`event handler <event>` that is triggered by the exit of the shell::


    function on_exit --on-event fish_exit
        echo fish is now exiting
    end

.. _auto-config:

The auto-config file
====================

The file ``~/.config/fish/config.auto.fish`` is referred to as the auto-config file, because it can be managed automatically by :ref:`fish_sync <cmd-fish_sync>` and is automatically re-run when it changes.

The file is not automatically re-run in scripts. The `fish_sync` command triggers a run manually.

It is safe to edit this file manually, but be aware of its re-run behavior.
