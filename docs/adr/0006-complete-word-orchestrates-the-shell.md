# `complete-word` orchestrates the target shell

`shcomp complete-word` does not implement completion. It prepares the cached script, starts the target shell under a controlled environment, lets the shell's real completion system register and run the function, captures candidates at `compadd`, and normalizes the output. A candidate-command protocol would only cover tools that expose one and could not answer for the vendor zsh scripts shcomp exists to manage. Because the harness is shell code, adding bash or fish later is a new harness, not a new completion engine.
