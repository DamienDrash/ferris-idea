#!/bin/sh
# A fake ACP agent for the tests of the agent host (SPEC-0348): no model, no network, no
# credentials. It answers the host's requests line by line and acts on the prompt text:
#   needs-approval  asks a permission request and reports the answer
#   max-tokens      ends the turn with stop reason max_tokens
#   fs              asks the host for fs/read_text_file and reports whether it was refused
#   env             reports whether variables of the test process reached it
#   garbage         writes a line that is no JSON-RPC and leaves the turn open
#   crash           exits with code 3
#   anything else   echoes the prompt
# The host's request ids are numbers and no other key of its requests is named `id`, so the
# `"id":` of a line is the request's id.

say() {
  printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"%s"}}}}\n' "$1"
}

answer() {
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"%s"}}\n' "$1" "$2"
}

# Reads lines until one carries `"id":<n>` and leaves it in $reply.
await() {
  while IFS= read -r reply; do
    case "$reply" in
      *"\"id\":$1,"* | *"\"id\":$1}"*) return 0 ;;
    esac
  done
  exit 0
}

while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":false},"authMethods":[]}}\n' "$id"
      ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"s1"}}\n' "$id"
      ;;
    *'"method":"session/prompt"'*)
      text=$(printf '%s' "$line" | sed -n 's/.*"text":"\([^"]*\)".*/\1/p')
      case "$text" in
        needs-approval)
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Write notes.txt","kind":"edit","status":"pending"}}}\n'
          printf '{"jsonrpc":"2.0","id":900,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t1","title":"Write notes.txt","kind":"edit"},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"always","name":"Always allow","kind":"allow_always"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}\n'
          await 900
          case "$reply" in
            *'"optionId":"allow"'*) say "approved"; answer "$id" end_turn ;;
            *'"cancelled"'*) answer "$id" cancelled ;;
            *) say "rejected"; answer "$id" end_turn ;;
          esac
          ;;
        write-files)
          printf 'hello\n' > notes.txt
          printf '\000\001' > Built.class
          say "wrote"; answer "$id" end_turn
          ;;
        edit-twice)
          printf '{"jsonrpc":"2.0","id":902,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t2","title":"Edit notes.txt","kind":"edit","content":[{"type":"diff","path":"notes.txt","oldText":"hello\\n","newText":"hello world\\n"}]},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}\n'
          await 902
          printf '{"jsonrpc":"2.0","id":903,"method":"session/request_permission","params":{"sessionId":"s1","toolCall":{"toolCallId":"t3","title":"Edit notes.txt again","kind":"edit","content":[{"type":"diff","path":"notes.txt","oldText":"hello\\n","newText":"hi\\n"}]},"options":[{"optionId":"allow","name":"Allow once","kind":"allow_once"},{"optionId":"reject","name":"Reject","kind":"reject_once"}]}}\n'
          await 903
          say "asked twice"; answer "$id" end_turn
          ;;
        max-tokens) say "partial"; answer "$id" max_tokens ;;
        fs)
          printf '{"jsonrpc":"2.0","id":901,"method":"fs/read_text_file","params":{"sessionId":"s1","path":"/etc/hosts"}}\n'
          await 901
          case "$reply" in
            *'"error"'*) say "fs-refused" ;;
            *) say "fs-served" ;;
          esac
          answer "$id" end_turn
          ;;
        env)
          say "cargo=${CARGO_MANIFEST_DIR:-unset} home=${HOME:+set}"
          answer "$id" end_turn
          ;;
        garbage) echo "this is not json" ;;
        crash) exit 3 ;;
        *) say "echo: $text"; answer "$id" end_turn ;;
      esac
      ;;
  esac
done
exit 0
