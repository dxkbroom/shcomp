# Broker, not a completion engine

shcomp asks a tool for its own completion script and caches it. It does not parse `--help`, synthesize a spec, or implement candidates itself. A completion engine is a different product (carapace-shaped) and would make every vendor quirk shcomp's bug; a broker keeps the tool's own knowledge authoritative and the crate small. `complete-word` is the exception in appearance only: it orchestrates the target shell, which still does the completing.
