# Copyright 2026 git-next contributors. MIT license.
__git_terminal_update_prompt() {
  local previous_status=$? path=${PWD//\\//} parent= gitdir= head= branch= label= directory= unc=
  if [[ -n ${GIT_DIR:-} ]]; then
    gitdir=$GIT_DIR
  else
    while [[ -n $path ]]; do
      directory=${path%/}/
      if [[ -d ${directory}.git ]]; then
        gitdir=${directory}.git
        break
      elif [[ -f ${directory}.git ]]; then
        IFS= read -r gitdir < "${directory}.git"
        gitdir=${gitdir%$'\r'}
        if [[ $gitdir == 'gitdir: '* ]]; then
          gitdir=${gitdir#gitdir: }
          [[ $gitdir == /* || $gitdir == [[:alpha:]]:* ]] || gitdir=$path/$gitdir
        else
          gitdir=
        fi
        break
      elif [[ -f ${directory}HEAD && -d ${directory}objects ]]; then
        gitdir=$path
        break
      fi
      [[ $path != / ]] || break
      [[ $path != [[:alpha:]]: && $path != /[[:alpha:]] ]] || break
      if [[ $path == //* ]]; then
        unc=${path#//}
        [[ ${unc#*/} == */* ]] || break
      fi
      parent=${path%/*}
      # Drive roots, UNC roots and malformed PWD values must always terminate.
      [[ $parent != "$path" ]] || break
      path=${parent:-/}
    done
  fi
  if [[ -n $gitdir && -r $gitdir/HEAD ]]; then
    IFS= read -r head < "$gitdir/HEAD"
    head=${head%$'\r'}
    if [[ $head == 'ref: refs/heads/'* ]]; then
      branch=${head#ref: refs/heads/}
    elif [[ $head != *[![:xdigit:]]* && ( ${#head} == 40 || ${#head} == 64 ) ]]; then
      branch=${head:0:7}
    fi
  fi
  branch=${branch//[[:cntrl:]]/?}
  __git_terminal_branch=${branch:+[$branch]}
  label=$PWD
  label+="${branch:+ [$branch]}"
  label=${label//[[:cntrl:]]/?}
  printf '\033]0;git-terminal-prompt:%s\007' "$label"
  return "$previous_status"
}

# Variable values are expanded by Bash without re-evaluating branch text as code.
PROMPT_DIRTRIM=${PROMPT_DIRTRIM:-3}
PS1='\[\e[36m\]\w\[\e[0m\] \[\e[35m\]${__git_terminal_branch}\[\e[0m\]\n\[\e[32m\]❯\[\e[0m\] '
PROMPT_COMMAND+=(__git_terminal_update_prompt)
