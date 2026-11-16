# The resolved executable path is the version key

shcomp resolves the command through the shell's own lookup and keys its cache by the executable's real path, mtime, and size. It does not ask mise or any manager which version is active on the completion path. Activation already writes the active install directory to `PATH`, so the path is the one truth that works under every manager and in every shell; mtime catches an in-place upgrade, and a version switch is a different path. mise is consulted only at install time, for discovery and prefetching.
