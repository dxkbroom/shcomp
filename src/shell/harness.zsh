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
  'fpath=(/usr/share/zsh/functions/Completion /usr/share/zsh/functions/Completion/*)'
  'autoload -Uz compinit'
  "compinit -u -d ${(q)tmp}/zcompdump"
  'autoload -Uz bashcompinit'
  'bashcompinit'
  "PROMPT=''"
  "RPROMPT=''"
  'COLUMNS=200'
  'LINES=50'
  'setopt no_always_last_prompt'
)
local command_line
for command_line in $setup; do
  zpty -w z "$command_line"
done

zpty -w z "source -- ${(q)script}"
zpty -w z 'typeset -ga __shcomp_matches'
zpty -w z 'typeset -gA __shcomp_seen __shcomp_descs'
zpty -w z 'compadd() { local -a __tmp __descs __args; __args=("$@"); local -i __i=1 __n; while (( __i <= $#__args )); do case $__args[$__i] in -d) __descs=("${(@P)__args[$((__i+1))]}"); (( __i += 2 ));; -a|-A|-D|-O|-P|-S|-p|-s|-i|-I|-W|-M|-J|-V|-X|-x|-r|-R|-F|-t) (( __i += 2 ));; -*) (( __i += 1 ));; *) break;; esac; done; builtin compadd -O __tmp "$@"; __n=${#__tmp}; local __word; for (( __i = 1; __i <= __n; __i++ )); do __word=${__tmp[$__i]}; [[ -n $__word ]] || continue; [[ -n ${__shcomp_seen[$__word]-} ]] && continue; __shcomp_seen[$__word]=1; __shcomp_matches+=("$__word"); [[ -n ${__descs[$__i]-} ]] && __shcomp_descs[$__word]=${__descs[$__i]}; done }'
zpty -w z "__shcomp_vendor=\${_comps[${(q)command}]-_${command}}"
zpty -w z 'functions[__shcomp_vendor_fn]=${functions[$__shcomp_vendor]}'
zpty -w z "__shcomp_probe() { __shcomp_matches=(); __shcomp_seen=(); __shcomp_descs=(); __shcomp_vendor_fn; local __word; for __word in \"\${__shcomp_matches[@]}\"; do print -r -- \"\$__word\"\$'\\t'\"\${__shcomp_descs[\$__word]-}\"; done > ${(q)out} 2>/dev/null; print -r -- __SHCOMP_DONE__ >> ${(q)out} }"
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
