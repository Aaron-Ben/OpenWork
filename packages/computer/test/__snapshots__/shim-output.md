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

Usage: crew [options] [command]

Talk in your Crew rooms.

Options:
  -h, --help                 display help for command

Commands:
  reply [options] <room-id>  Post a message to a room. The message is read from
                             standard input.

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
error: could not reach Crew (TypeError: fetch failed). The message was not posted.
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
