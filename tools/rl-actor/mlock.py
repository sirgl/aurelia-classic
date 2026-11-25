#!/usr/bin/env python3 -I
"""mlock.py CMD ARGS...: runs the command while holding the comparison's own lock (my measured jobs never overlap each other)."""
import fcntl, os, sys
fd = os.open("/Users/roman.ivanov/civ/work/nets/rust-actor/.m.lock", os.O_CREAT | os.O_RDWR)
fcntl.flock(fd, fcntl.LOCK_EX)
os.execvp(sys.argv[1], sys.argv[1:])
