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

# A #compdef file is not always an autoloadable function body.  It may be a
# script that defines completion functions and registers them with compdef at
# top level (bun, cargo, gh, and most generated scripts do this), or it may be
# a plain function body whose text is the completion function (`_eza` and
# `_gradle` are).  Autoloading the first kind only executes its definitions:
# the first call registers the callback and returns no matches, so no
# completions are emitted.  Source a script that registers itself or that
# guards a self-call on `funcstack`; keep a plain function body in fpath so
# compinit can autoload it.
local use_fpath=0
if head -n 1 -- "$script" 2>/dev/null | grep -q '^#compdef'; then
  if grep -Eq '^compdef |funcstack\[1\]' "$script" 2>/dev/null; then
    use_fpath=0
  else
    use_fpath=1
    mkdir -p -- "$tmp/functions"
    cp -- "$script" "$tmp/functions/_$command"
  fi
fi

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
  "fpath=(${(q)tmp}/functions /usr/share/zsh/functions/* /usr/share/zsh/functions/Completion/*)"
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
# compadd is a builtin; a function of the same name shadows it, so every call
# from _arguments, _alternative, or _describe reaches this capture.  Passing
# -O asks zsh to return the matches instead of adding them, while still
# applying its real prefix and matcher filtering.  The display arrays (-D and
# -d) keep the descriptions the vendor supplied.
zpty -w z 'compadd() { local -a __args __matches __D __d; local __arg __word __entry __val __desc __orig=; local -i __i __n __opts=1; __args=("$@"); for (( __i = 1; __i <= $#__args; __i++ )); do __arg=$__args[$__i]; if (( __opts )); then case $__arg in --) __opts=0;; -D) __D+=("${__args[$((__i+1))]}");; -d) __d+=("${__args[$((__i+1))]}");; -O) __orig=${__args[$((__i+1))]};; esac; fi; done; if [[ -n $__orig ]]; then builtin compadd "$@"; __matches=("${(@P)__orig}"); else builtin compadd -O __matches "$@"; fi; __n=${#__matches}; for (( __i = 1; __i <= __n; __i++ )); do __word=${__matches[$__i]}; [[ -n $__word ]] || continue; [[ -n ${__shcomp_seen[$__word]-} ]] && continue; __shcomp_seen[$__word]=1; __shcomp_matches+=("$__word"); for __arg in "${(@)__D}"; do for __entry in "${(@P)__arg}"; do __val=${__entry%%:*}; __desc=${__entry#*:}; __val=${__val//\\:/:}; __desc=${__desc//\\:/:}; if [[ $__val == $__word ]]; then [[ -n $__desc && $__desc != $__val ]] && __shcomp_descs[$__word]=$__desc; continue 3; fi; done; done; for __arg in "${(@)__d}"; do for __entry in "${(@P)__arg}"; do [[ $__entry == *" -- "* ]] || continue; __val=${__entry%%[[:space:]]*}; __desc=${__entry#* -- }; if [[ $__val == $__word ]]; then [[ -n $__desc ]] && __shcomp_descs[$__word]=$__desc; continue 3; fi; done; done; done; }'

# An autoloaded vendor function may only define its completion body on the
# first call.  The probe calls it again if the autoload produced no matches,
# and then follows any compdef the script registered.
zpty -w z "__shcomp_vendor=\${_comps[${(q)command}]-_${command}}"
zpty -w z "[[ \$+functions[\$__shcomp_vendor] -eq 0 ]] && __shcomp_vendor=_${command}"
zpty -w z "__shcomp_probe() { __shcomp_matches=(); __shcomp_seen=(); __shcomp_descs=(); local __autoload=0; [[ \${functions[\$__shcomp_vendor]-} == *autoload* ]] && __autoload=1; \"\$__shcomp_vendor\"; (( __autoload && ! \${#__shcomp_matches} )) && \"\$__shcomp_vendor\"; local __after=\${_comps[${(q)command}]-}; if [[ -n \$__after && \$__after != \$__shcomp_vendor && \$__after != __shcomp_probe ]]; then \"\$__after\"; fi; local __word; for __word in \"\${__shcomp_matches[@]}\"; do print -r -- \"\$__word\"\$'\\t'\"\${__shcomp_descs[\$__word]-}\"; done > ${(q)out} 2>/dev/null; print -r -- __SHCOMP_DONE__ >> ${(q)out} }"
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
