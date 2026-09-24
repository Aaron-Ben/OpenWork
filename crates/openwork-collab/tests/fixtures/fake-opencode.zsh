#!/bin/zsh

prompt="$(cat)"

if [[ "$prompt" == "continue the work" ]]; then
  print -r -- '{"type":"step_start","sessionID":"ses_local"}'
  print -r -- '{"type":"text","sessionID":"ses_local","part":{"text":"done"}}'
  print -r -- '{"type":"step_finish","sessionID":"ses_local","part":{"tokens":{"input":11,"output":3,"reasoning":2,"cache":{"read":7,"write":5}}}}'
  exit 0
fi

if [[ " $* " == *" --agent openwork-triage "* ]]; then
  print -r -- '{"type":"text","part":{"text":"{\"actionable\":false,\"reason\":\"agent-only noise\",\"promptNote\":\"\"}"}}'
  print -r -- '{"type":"step_finish","part":{"tokens":{"input":4,"output":2,"cache":{"read":0,"write":0}}}}'
  exit 0
fi

# 卡片 Turn（collaboration.md §11.4）：领取列表里的第一张卡片。
if [[ "$prompt" == *"Someone just put this work on you directly."* ]]; then
  card_id="$(print -r -- "$prompt" | sed -n 's/^- \(card-[0-9a-f]*\) .*/\1/p' | head -n 1)"
  openwork card claim "$card_id" >/dev/null || exit $?
  print -r -- 'Claimed by the card turn; see "notes".' | openwork card update "$card_id" --stdin >/dev/null || exit $?
  print -r -- '{"type":"text","sessionID":"ses_helper","part":{"text":"claimed"}}'
  print -r -- '{"type":"step_finish","sessionID":"ses_helper","part":{"tokens":{"input":8,"output":3,"cache":{"read":1,"write":0}}}}'
  exit 0
fi

room_id="$(print -r -- "$prompt" | sed -n 's/^# \(room-[0-9a-f]*\).*/\1/p' | head -n 1)"
printf '%s\n%s' 'Agent says `code` $(literal) --as=admin' 'second line' \
  | openwork reply "$room_id" --stdin >/dev/null || exit $?
session_id="ses_helper"
print -r -- "{\"type\":\"text\",\"sessionID\":\"$session_id\",\"part\":{\"text\":\"published\"}}"
print -r -- "{\"type\":\"step_finish\",\"sessionID\":\"$session_id\",\"part\":{\"tokens\":{\"input\":8,\"output\":3,\"cache\":{\"read\":1,\"write\":0}}}}"
