.. _cmd-fish_sync:

fish_sync - modify and rerun the auto configuration file
=============================================================================

Synopsis
--------

::

    fish_sync [(-v | --var) VARIABLE ] ...


Description
-----------

``fish_sync`` will re-run the :ref:`automatic config <auto-config>` file, if it has changed since the last time it was run. ``fish_sync`` may also be used to modify the file, by saving the named variables into it.

The following options are available:

- ``-v VARIABLE`` or ``--var VARIABLE`` will save the value of the variable into the auto config, by writing a :ref:`set <cmd-set>` command to the file. If there is already such a `set` command, it is replaced. If the variable is not set, the `set` command is deleted. The variable is always saved as global.
