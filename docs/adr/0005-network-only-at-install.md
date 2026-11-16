# URL sources are fetched at install, never at completion

`url` sources are downloaded and verified only by `shcomp install`. `load` and Tab completion read the cache and fail with "run `shcomp install`" on a miss. Fetching during completion would put network latency and a remote failure mode on every cold Tab, and would let a version switch silently reach the network. Install enumerates installed versions through mise when it can, so switching projects stays offline.
