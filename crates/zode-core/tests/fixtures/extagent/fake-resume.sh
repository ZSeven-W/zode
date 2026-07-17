#!/bin/sh
# fake resumable CLI: emit a session id on first run; on --resume, emit resumed-ok
consumed=$(cat)
if echo "$@" | grep -q -- "--resume"; then
  printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-A"}'
  printf '%s\n' '{"type":"result","subtype":"success","result":"resumed-ok","session_id":"sess-A"}'
else
  printf '%s\n' '{"type":"system","subtype":"init","session_id":"sess-A"}'
  printf '%s\n' '{"type":"result","subtype":"success","result":"first-run done","session_id":"sess-A"}'
fi
