#!/usr/bin/env bash
# Open the agentic console in tmux.
#
#   ./open.sh [repo-path]           attach to the console's session, starting it if needed
#   ./open.sh --reload [repo-path]  relaunch the console IN PLACE on the current binary,
#                                   without destroying the session you are attached to
#
# With no repo-path it watches the repository this crate ships inside (console/../), so a fork of the
# kit builds it and runs it without arguments. A standalone clone has no kit above it, so it is told
# to name the repository rather than silently watching the wrong directory.
# The session lives on its own tmux SOCKET (agentic-console), not the default one. That matters:
# anything else doing `tmux kill-server` or `tmux kill-session -t ac` on the default socket cannot
# reach this session, so it survives other tools' cleanup and any agent that tidies up after itself.
# The cost is that the plain attach command needs the socket name:
#
#   attach          tmux -L agentic-console attach -t ac        (or just run ./open.sh)
#   leave running   Ctrl-b then d                               (detach; the console keeps running)
#   quit the app    q  (or Ctrl-C)                              (you land at a shell IN the session)
#   end the session exit                                        (at that shell)
#   relaunch in place  ./open.sh --reload                       (never kills your session)
#
# A login shell owns the session and the console is sent to it as a keystroke, so the app quitting
# - on purpose or by crashing - leaves the session alive at a prompt instead of tearing it down.

set -u

SOCKET=agentic-console
SESSION=ac
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"     # the console crate: this directory
BIN="$HERE/target/release/agentic-console"
# The console ships inside the kit, so a fork that clones the kit builds this crate and runs
# console/open.sh with no argument: HERE/.. is the kit, and it carries agentic.config.json.
# A standalone clone has no kit above it -- HERE/.. is only wherever you cloned into -- so that
# default is taken only when it actually looks like a kit repository, and otherwise the console
# says what it is watching and how to name the repository instead.
if [ -f "$HERE/../agentic.config.json" ]; then
  REPO_DEFAULT="$(cd "$HERE/.." && pwd)"
else
  REPO_DEFAULT="$PWD"
  echo "note: no agentic.config.json above this crate, so the console will watch $REPO_DEFAULT" >&2
  echo "      name the repository instead: ./open.sh /path/to/repo" >&2
fi
T=(tmux -L "$SOCKET")

RELOAD=0
if [ "${1:-}" = "--reload" ]; then
  RELOAD=1
  shift
fi
REPO="${1:-$REPO_DEFAULT}"

if [ ! -x "$BIN" ]; then
  echo "no console binary at $BIN" >&2
  echo "build it with:  cd $HERE && cargo build --release" >&2
  exit 1
fi
if [ ! -d "$REPO" ]; then
  echo "no such repository: $REPO" >&2
  exit 1
fi

# NOTE: no `exec` here on purpose. The session's own process must stay the shell, so that the
# console is its child: when the app exits - quit, crash, or a --reload - the shell is still there
# and the session survives. `exec` would replace the shell with the console and the session would
# die with the app, which is exactly the failure this script exists to prevent.
run_line="cd $HERE && $BIN --repo $REPO"

if "${T[@]}" has-session -t "$SESSION" 2>/dev/null; then
  if [ "$RELOAD" = 1 ]; then
    # Quit the app with Ctrl-C (it quits from any mode), leaving the owning shell, then run it again.
    # The session is never destroyed, so an attached operator keeps their terminal.
    "${T[@]}" send-keys -t "$SESSION" C-c
    sleep 1
    "${T[@]}" send-keys -t "$SESSION" "$run_line" Enter
    echo "relaunched the console in session $SOCKET:$SESSION on the current binary"
  else
    echo "attaching to session $SOCKET:$SESSION (detach with Ctrl-b d)"
  fi
  exec "${T[@]}" attach -t "$SESSION"
fi

if [ "$RELOAD" = 1 ]; then
  echo "no session $SOCKET:$SESSION to reload; starting it instead"
fi

# A login shell owns the session, so the session outlives the console process.
"${T[@]}" new-session -d -s "$SESSION" -x 205 -y 54
"${T[@]}" send-keys -t "$SESSION" \
  "$run_line; echo; echo '[console exited - this shell keeps the session alive; ./open.sh --reload relaunches it]'" Enter
exec "${T[@]}" attach -t "$SESSION"
