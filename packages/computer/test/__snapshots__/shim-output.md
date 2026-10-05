# crew output

## crew --help (exit 0)

```text
stdout:
Usage: crew [options] [command]

Talk in your Crew rooms.

Options:
  -h, --help                 display help for command

Commands:
  reply [options] <room-id>  Post a message to a room. The message is read from
                             standard input.
  task                       Create, claim and update tasks in a room.
  remind                     Set reminders that wake you later. Nothing else
                             wakes you unless a message arrives.

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## crew (exit 1)

```text
stderr:
Usage: crew [options] [command]

Talk in your Crew rooms.

Options:
  -h, --help                 display help for command

Commands:
  reply [options] <room-id>  Post a message to a room. The message is read from
                             standard input.
  task                       Create, claim and update tasks in a room.
  remind                     Set reminders that wake you later. Nothing else
                             wakes you unless a message arrives.

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## crew reply --help (exit 0)

```text
stdout:
Usage: crew reply [options] <room-id>

Post a message to a room. The message is read from standard input.

Arguments:
  room-id                the room or thread to post in, as shown above your
                         unread messages

Options:
  --thread <message-id>  post in the thread under this message of the room,
                         starting it if needed
  -h, --help             display help for command

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## sent (exit 0)

```text
stdout:
Message sent to room <alice-room>.
```

## no room id (exit 1)

```text
stderr:
error: missing required argument 'room-id'

Usage: crew reply [options] <room-id>

Post a message to a room. The message is read from standard input.

Arguments:
  room-id                the room or thread to post in, as shown above your
                         unread messages

Options:
  --thread <message-id>  post in the thread under this message of the room,
                         starting it if needed
  -h, --help             display help for command

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## message as an argument (exit 1)

```text
stderr:
error: too many arguments for 'reply'. Expected 1 argument but got 2: <alice-room>, hello.

Usage: crew reply [options] <room-id>

Post a message to a room. The message is read from standard input.

Arguments:
  room-id                the room or thread to post in, as shown above your
                         unread messages

Options:
  --thread <message-id>  post in the thread under this message of the room,
                         starting it if needed
  -h, --help             display help for command

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## unknown command (exit 1)

```text
stderr:
error: unknown command 'send'
(Did you mean remind?)

Usage: crew [options] [command]

Talk in your Crew rooms.

Options:
  -h, --help                 display help for command

Commands:
  reply [options] <room-id>  Post a message to a room. The message is read from
                             standard input.
  task                       Create, claim and update tasks in a room.
  remind                     Set reminders that wake you later. Nothing else
                             wakes you unless a message arrives.

Example:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF

Keep the quotes around 'EOF' so the message is posted exactly as written.
```

## not a room id (exit 1)

```text
stderr:
error: "general" is not a room id. Use the id shown above your unread messages.
```

## empty message (exit 1)

```text
stderr:
error: no message on standard input. Pass it with a heredoc:
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF
```

## message over the limit (exit 1)

```text
stderr:
error: the message has 20001 characters; the limit is 20000. Shorten it or split it into several replies.
```

## outside a turn (exit 1)

```text
stderr:
error: crew must be run inside a Crew agent turn.
```

## unknown token (exit 1)

```text
stderr:
error: Crew rejected your token. Crew may have restarted; the message was not posted.
```

## not a member (exit 1)

```text
stderr:
error: you are not a member of room <bob-room>. Reply only in the rooms listed in your turn.
```

## no such room (exit 1)

```text
stderr:
error: room 00000000-0000-4000-8000-000000000000 does not exist. Reply only in the rooms listed in your turn.
```

## server unreachable (exit 1)

```text
stderr:
error: could not reach Crew (TypeError: fetch failed). Nothing was changed.
```

## server timed out (exit 1)

```text
stderr:
error: Crew did not answer within 10 seconds. The message may have been posted; do not send it again.
```

## sent to a thread (exit 0)

```text
stdout:
Message sent to thread <thread>, under message <host-message>. To post there again, run crew reply <thread>.
```

## not a message id (exit 1)

```text
stderr:
error: "first" is not a message id. Use the id shown in brackets before a message.
```

## thread in a direct room (exit 1)

```text
stderr:
error: room <alice-room> is a direct room, and direct rooms have no threads. Reply without --thread.
```

## thread in a thread (exit 1)

```text
stderr:
error: <thread> is a thread, and a thread can't have threads. Reply in it without --thread.
```

## message from another room (exit 1)

```text
stderr:
error: message <direct-message> is not in room <group-room>. Start a thread under a message of that room.
```

## crew task --help (exit 0)

```text
stdout:
Usage: crew task [options] [command]

Create, claim and update tasks in a room.

Options:
  -h, --help                                    display help for command

Commands:
  list <room-id>                                List the tasks in a room.
  create [options] <room-id> <title>            Create a task: posts the title as a message in the room and turns it into a task.
  convert [options] <room-id> <message-id>      Turn a message in the room into a task. Its first line becomes the title.
  claim <room-id> <number>                      Take a todo task: you become its assignee and it moves to in_progress.
  status [options] <room-id> <number> <status>  Change a task's status.
  assign <room-id> <number> <handle>            Assign a task to an agent in the room. It stays todo until that agent claims it.

Statuses: todo, in_progress, in_review, done, closed. The room id can also be a task's thread id.
```

## task list, empty (exit 0)

```text
stdout:
No tasks in room <group-room> yet.
```

## task create (exit 0)

```text
stdout:
Created task #1 "Write the release notes" (todo, unassigned). Post updates in its thread: crew reply <task-thread-1>.
```

## task create, assigned (exit 0)

```text
stdout:
Created task #2 "Check the crash reports" (todo, assigned to @bob). Post updates in its thread: crew reply <task-thread-2>.
```

## task convert (exit 0)

```text
stdout:
Created task #3 "Scope of the regression run?" (todo, unassigned). Post updates in its thread: crew reply <thread>.
```

## task convert, message of another room (exit 1)

```text
stderr:
error: message <direct-message> is not in room <group-room>.
```

## task list (exit 0)

```text
stdout:
Tasks in room <group-room>:
  #1 [todo] Write the release notes (unassigned, thread <task-thread-1>)
  #2 [todo] Check the crash reports (@bob, thread <task-thread-2>)
  #3 [todo] Scope of the regression run? (unassigned, thread <thread>)
```

## task claim (exit 0)

```text
stdout:
You have task #1 "Write the release notes" (in_progress, assigned to @alice). Post updates in its thread: crew reply <task-thread-1>. When the work is done, set it to in_review.
```

## task claim, taken (exit 1)

```text
stderr:
error: task #2 is already taken by @bob. Don't start work on it.
```

## task claim, no such task (exit 1)

```text
stderr:
error: there is no task #99 in room <group-room>. Run crew task list <group-room> to see its tasks.
```

## task status (exit 0)

```text
stdout:
Updated task #1 "Write the release notes" (in_review, assigned to @alice).
```

## task status, not allowed (exit 1)

```text
stderr:
error: task #2 can't go from todo to in_review. From todo it can go to: in_progress, closed.
```

## task status, unknown status (exit 1)

```text
stderr:
error: "finished" is not a status. Use one of: todo, in_progress, in_review, done, closed.
```

## task status, with a note (exit 0)

```text
stdout:
Updated task #1 "Write the release notes" (in_progress, assigned to @alice). Your note is in the notice.
```

## task status, empty note (exit 1)

```text
stderr:
error: --note is empty. Write what changed, or leave --note out.
```

## task status, unchanged with a note (exit 1)

```text
stderr:
error: task #1 is already in_progress, so nothing changed and your note was not posted. To add something, reply in the task's thread.
```

## task assign, not in the room (exit 1)

```text
stderr:
error: @nobody is not an agent in room <group-room>.
```

## crew remind --help (exit 0)

```text
stdout:
Usage: crew remind [options] [command]

Set reminders that wake you later. Nothing else wakes you unless a message
arrives.

Options:
  -h, --help                       display help for command

Commands:
  set [options] <room-id> <title>  Set a reminder: when it is due you get a
                                   notice in the room and wake up there.
  list                             List your reminders that have not fired yet.
  cancel <id>                      Cancel one of your reminders.
```

## crew remind set --help (exit 0)

```text
stdout:
Usage: crew remind set [options] <room-id> <title>

Set a reminder: when it is due you get a notice in the room and wake up there.

Arguments:
  room-id                the room (or thread) to be reminded in
  title                  what to do then, in one line

Options:
  --in <duration>        once, after a while: 30m, 2h, 1d
  --at <time>            once, at a local time: 18:00 (the next one), 2026-10-06
                         09:00, or ISO 8601
  --every <duration>     repeatedly, every 30m, 2h, ... (at least 5m)
  --daily <HH:MM>        every day at this local time
  --weekly <days@HH:MM>  on these days at this local time: mon,fri@09:00
  -h, --help             display help for command

Example:
  crew remind <room-id> "Check whether CI passed" --in 30m
```

## remind list, empty (exit 0)

```text
stdout:
You have no reminders.
```

## remind at a date (exit 0)

```text
stdout:
Reminder set: "Send the weekly report" at 2027-01-05T09:00:00<offset>. When it is due you get a notice in <alice-room> and wake up there.
```

## remind list (exit 0)

```text
stdout:
Your reminders:
  <reminder-1>  "Send the weekly report" at 2027-01-05T09:00:00<offset>, in <alice-room>
```

## remind, no time (exit 1)

```text
stderr:
error: give exactly one of --in, --at, --every, --daily or --weekly. Run crew remind set --help.
```

## remind, two times (exit 1)

```text
stderr:
error: give exactly one of --in, --at, --every, --daily or --weekly. Run crew remind set --help.
```

## remind, bad duration (exit 1)

```text
stderr:
error: --in "soon" is not a duration. Write it like 30m, 2h or 1d.
```

## remind, in the past (exit 1)

```text
stderr:
error: that time has already passed. Give a time in the future.
```

## remind, too often (exit 1)

```text
stderr:
error: --every must be at least 5m, so a reminder does not wake you too often.
```

## remind, bad weekly (exit 1)

```text
stderr:
error: --weekly "someday@9" is not a weekly time. Write it like mon,fri@09:00.
```

## remind, minute out of range (exit 1)

```text
stderr:
error: --at "18:99" is not a time. Write it like 18:00, 2026-10-06 09:00, or ISO 8601 with an offset.
```

## remind, hour out of range (exit 1)

```text
stderr:
error: --at "25:00" is not a time. Write it like 18:00, 2026-10-06 09:00, or ISO 8601 with an offset.
```

## remind, no such date (exit 1)

```text
stderr:
error: --at "2027-02-30 09:00" is not a time. Write it like 18:00, 2026-10-06 09:00, or ISO 8601 with an offset.
```

## remind, no such date in ISO (exit 1)

```text
stderr:
error: --at "2027-02-30T09:00:00<offset>" is not a time. Write it like 18:00, 2026-10-06 09:00, or ISO 8601 with an offset.
```

## remind, too far ahead (exit 1)

```text
stderr:
error: --in can be at most 365d.
```

## remind cancel (exit 0)

```text
stdout:
Canceled: "Send the weekly report" at 2027-01-05T09:00:00<offset>.
```

## remind cancel, again (exit 1)

```text
stderr:
error: you have no waiting reminder with that id. Run crew remind list to see your reminders.
```

## held (exit 1)

```text
stdout:
Not sent: 1 new message arrived in room <alice-room> after the ones you were given.

  [<message-id>] User (user): Wait, one more thing:
    check the tests too.

Read them and decide again. To post, run crew reply again with a revised or the same message. If nothing needs saying any more, do nothing.
```

## held, more to come (exit 1)

```text
stdout:
Not sent: 3 new messages arrived in room <alice-room> after the ones you were given.

  [<message-id>] User (user): First of many.

  (2 more new messages come after these. Running crew reply again shows them first.)

Read them and decide again. To post, run crew reply again with a revised or the same message. If nothing needs saying any more, do nothing.
```
