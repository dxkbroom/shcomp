#!/usr/bin/env zsh
# Ask a real interactive zsh for completion candidates.
#
# Args: script command line out-file timeout-ms
# The inner shell runs the vendor completion function in a real ZLE
# completion context; compadd is intercepted with -O and its matches are
# written to the output file. Nothing here reimplements completion.

emulate -R zsh

local script=$1 command=$2 line=$3 out=$4 timeout_ms=$5
local tmp=${out:h}
[[ -d $tmp ]] || mkdir -p -- "$tmp"

# A #compdef file is an autoload function, not a script to source: its body
# may define a helper and call it at the top level. Put it in an fpath
# directory so compinit autoloads it at completion time.
local use_fpath=0
if head -n 1 -- "$script" 2>/dev/null | grep -q '^#compdef'; then
  use_fpath=1
  mkdir -p -- "$tmp/functions"
  cp -- "$script" "$tmp/functions/_$command"
fi

# Write our _describe interceptor to the fpath directory BEFORE compinit so it
# shadows the original.  Vendor completion scripts (e.g., bun via _alternative)
# call _describe through zsh's autoload system, which resolves names via fpath.
# By placing our interceptor first in fpath, every autoloaded call to _describe
# reaches us instead of the original.  compadd is a widget, not autoload, so it
# cannot be shadowed this way — we still need the zpty-based compadd override.
mkdir -p -- "$tmp/functions"
cat > "$tmp/functions/_describe" << 'DESCRIBE_EOF'
#autoload
# _describe [options] tag description array_name [more_options]
# Parse options to find the array name, then extract entries.
local tag descr varname
while [[ "$1" == -* ]]; do
  case "$1" in
    -t) tag=$2; shift 2;;
    -M) shift 2;;  # skip match pattern
    -o|-O|-1|-2|-J|-V|-x) shift;;
    *) shift;;
  esac
done
descr=$1; shift
varname=$1; shift
local -a __entries
__entries=("${(@P)varname}")
local __entry __val __desc
for __entry in $__entries; do
  __val=${__entry%%:*}
  __desc=${__entry#*:}
  if [[ $__desc != $__val ]]; then
    __shcomp_descs[$__desc]=$__desc
  fi
  [[ -n ${__shcomp_seen[$__val]-} ]] || {
    __shcomp_seen[$__val]=1
    __shcomp_matches+=("$__val")
  }
  if [[ $__desc != $__val && -n $__desc ]]; then
    __shcomp_descs[$__val]=$__desc
  fi
done
return 0
DESCRIBE_EOF

# Intercept _arguments so scripts that use `_arguments ... && ret=0` (e.g., bun)
# still reach their case block.  _arguments returns 1 when it has more work to do,
# but in our harness the vendor function is called directly, so the && short-circuits
# and the case block is skipped.  Our wrapper always returns 0 so the vendor
# function continues to the case $state block.
cat > "$tmp/functions/_arguments" << 'ARGUMENTS_EOF'
#autoload
__shcomp_real_arguments "$@"
return 0
ARGUMENTS_EOF
cp /usr/share/zsh/functions/Completion/Base/_arguments "$tmp/functions/__shcomp_real_arguments" 2>/dev/null

# Intercept _alternative so scripts that use it (e.g., bun) route through our
# _describe/compadd interceptors.  _alternative dispatches to _describe for
# ((...)) patterns and compadd for (...) patterns.  By saving the real
# _alternative and re-invoking it after our overrides are loaded, those calls
# reach our interceptors.
cat > "$tmp/functions/_alternative" << 'ALTERNATIVE_EOF'
#autoload
__shcomp_real_alternative "$@"
return $?
ALTERNATIVE_EOF
cp /usr/share/zsh/functions/Completion/Base/_alternative "$tmp/functions/__shcomp_real_alternative" 2>/dev/null

zmodload zsh/zpty 2>/dev/null || {
  print -u2 -- "zsh/zpty is unavailable"
  exit 1
}
zmodload zsh/zselect 2>/dev/null || {
  print -u2 -- "zsh/zselect is unavailable"
  exit 1
}

zpty -b z zsh -f -i || {
  print -u2 -- "cannot start the probe shell"
  exit 1
}

local -a setup
setup=(
  'unset FPATH'
  "fpath=(${(q)tmp}/functions /usr/share/zsh/functions/Completion /usr/share/zsh/functions/Completion/*)"
  'autoload -Uz compinit'
  "compinit -u -d ${(q)tmp}/zcompdump"
  'autoload -Uz bashcompinit'
  'bashcompinit'
  "PROMPT=''"
  "RPROMPT=''"
  'COLUMNS=200'
  'LINES=50'
  'setopt no_always_last_prompt'
  # The same guard the shim sets, so a probe answers what a real Tab answers:
  # vendor scripts that leave an `_arguments` exclusion group unquoted (gradle
  # does) would otherwise abort here while succeeding in the installed shell.
  'setopt no_nomatch'
)
local command_line
for command_line in $setup; do
  zpty -w z "$command_line"
done

if (( ! use_fpath )); then
  zpty -w z "source -- ${(q)script}"
fi
zpty -w z 'typeset -ga __shcomp_matches'
zpty -w z 'typeset -gA __shcomp_seen __shcomp_descs'
# Force-load our _describe from fpath before any other completion function
# (like _alternative) autoloads and pins _describe to the system path.
# Once loaded, _describe stays as a regular function and cannot be overridden
# by a later autoload, so _alternative will use our version.
zpty -w z 'autoload -Uz _describe'
# Load the real _arguments (saved as __shcomp_real_arguments) after our
# _describe is loaded, so it resolves _describe to our version.
zpty -w z 'autoload -Uz __shcomp_real_arguments'
zpty -w z 'autoload -Uz _arguments'
# Load the real _alternative (saved as __shcomp_real_alternative) after our
# _describe is loaded, so it resolves _describe to our version.
zpty -w z 'autoload -Uz __shcomp_real_alternative'
zpty -w z 'autoload -Uz _alternative'
# compadd is a completion widget, not an autoload function, so it cannot be
# shadowed via fpath.  Define it as a regular function in the zpty shell so
# direct compadd calls (e.g., from _alternative with (...) patterns) reach us.
zpty -w z 'compadd() { local -a __tmp __descs __args; __args=("$@"); local -i __i=1 __n; while (( __i <= $#__args )); do case $__args[$__i] in -d) __descs=("${(@P)__args[$((__i+1))]}"); (( __i += 2 ));; -a|-A|-D|-O|-P|-S|-p|-s|-i|-I|-W|-M|-J|-V|-X|-x|-r|-R|-F|-t) (( __i += 2 ));; -*) (( __i += 1 ));; *) break;; esac; done; builtin compadd -O __tmp "$@"; __n=${#__tmp}; local __word; for (( __i = 1; __i <= __n; __i++ )); do __word=${__tmp[$__i]}; [[ -n $__word ]] || continue; [[ -n ${__shcomp_seen[$__word]-} ]] && continue; __shcomp_seen[$__word]=1; __shcomp_matches+=("$__word"); [[ -n ${__descs[$__i]-} ]] && __shcomp_descs[$__word]=${__descs[$__i]}; done }'
zpty -w z "__shcomp_vendor=\${_comps[${(q)command}]-_${command}}"
zpty -w z "__shcomp_probe() { __shcomp_matches=(); __shcomp_seen=(); __shcomp_descs=(); \"\$__shcomp_vendor\"; local __after=\${_comps[${(q)command}]-}; if [[ -n \$__after && \$__after != \$__shcomp_vendor && \$__after != __shcomp_probe ]]; then \"\$__after\"; fi; local __word; for __word in \"\${__shcomp_matches[@]}\"; do print -r -- \"\$__word\"\$'\\t'\"\${__shcomp_descs[\$__word]-}\"; done > ${(q)out} 2>/dev/null; print -r -- __SHCOMP_DONE__ >> ${(q)out} }"
zpty -w z "compdef __shcomp_probe ${(q)command}"

# The line is queued behind the setup commands; ZLE reads it only after they
# finish. A tab starts completion, which runs the vendor function in a real
# completion context.
zpty -w -n z "$line"$'\t'

local log=
local chunk=
local -i waited=0
while (( waited < timeout_ms )); do
  if [[ -f $out ]] && grep -q '^__SHCOMP_DONE__$' "$out" 2>/dev/null; then
    break
  fi
  while zpty -rt z chunk 2>/dev/null; do
    log+=$chunk
  done
  zselect -t 1
  (( waited += 10 ))
done
zpty -d z 2>/dev/null

if [[ -f $out ]] && grep -q '^__SHCOMP_DONE__$' "$out" 2>/dev/null; then
  grep -v '^__SHCOMP_DONE__$' "$out"
  exit 0
fi

print -u2 -- "completion did not finish"
print -u2 -- "$log"
exit 1
