# Copyright 2026 git-next contributors. MIT license.
__git_terminal_update_prompt() {
  local previous_status=$? path=$PWD found= branch= label=
  if [[ -n ${GIT_DIR:-} ]]; then
    found=1
  else
    while [[ -n $path ]]; do
      if [[ -e $path/.git || ( -f $path/HEAD && -d $path/objects ) ]]; then
        found=1
        break
      fi
      [[ $path != / ]] || break
      path=${path%/*}
      [[ -n $path ]] || path=/
    done
  fi
  if [[ $found && -x ${GIT_NEXT_EXECUTABLE:-} ]]; then
    branch=$("$GIT_NEXT_EXECUTABLE" -C "$PWD" symbolic-ref --quiet --short HEAD 2>/dev/null) ||
      branch=$("$GIT_NEXT_EXECUTABLE" -C "$PWD" rev-parse --short HEAD 2>/dev/null) || branch=
  fi
  __git_terminal_branch=${branch:+[$branch]}
  label=${PWD##*/}
  [[ $label ]] || label=/
  label+="${branch:+ [$branch]}"
  label=${label//[[:cntrl:]]/?}
  printf '\033]0;%s\007' "$label"
  return "$previous_status"
}

# Variable values are expanded by Bash without re-evaluating branch text as code.
PS1='\[\e[36m\]\w\[\e[0m\] \[\e[35m\]${__git_terminal_branch}\[\e[0m\]\n\[\e[32m\]❯\[\e[0m\] '
if [[ $(declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a '* ]]; then
  PROMPT_COMMAND+=(__git_terminal_update_prompt)
elif [[ ${PROMPT_COMMAND:-} ]]; then
  PROMPT_COMMAND+=(__git_terminal_update_prompt)
else
  PROMPT_COMMAND=(__git_terminal_update_prompt)
fi
