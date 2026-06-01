find / -type f -name '*.key' 2>/dev/null | \
  xargs grep -l 'token' 2>/dev/null | \
  head -22
