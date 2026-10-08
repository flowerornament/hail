"""A stand-in for Codex's composer in the scenarios: it reads the terminal raw
and, like Codex's paste-burst handling, treats an Enter that arrives in the
same read as typed text as a newline. It shows a line from an earlier
envelope first and stops reading for STALL seconds, as a busy agent does.
Then it prints SUBMITTED <text> or COMPOSER <text> for each line."""
import os, sys, termios, tty, time
stale, stall = sys.argv[1], float(sys.argv[2])
fd = sys.stdin.fileno()
tty.setraw(fd)
os.write(1, (stale + "\r\n").encode())
time.sleep(stall)
buf = b""
while True:
    chunk = os.read(fd, 4096)
    if not chunk:
        break
    # Echo what was typed, as a composer shows it.
    os.write(1, chunk.replace(b"\r", b""))
    for b in chunk:
        if b == 13:
            # Enter in the same read as text is part of a paste: a newline.
            verdict = "COMPOSER" if len(chunk) > 1 else "SUBMITTED"
            os.write(1, f"\r\n{verdict} {buf.decode(errors='replace')}\r\n".encode())
            buf = b""
        else:
            buf += bytes([b])
