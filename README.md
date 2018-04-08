
Use as a non-root user:

/proc/sys/kernel/perf_event_paranoid

the error message that perf would show:

```
You may not have permission to collect stats.
Consider tweaking /proc/sys/kernel/perf_event_paranoid:
 -1 - Not paranoid at all
  0 - Disallow raw tracepoint access for unpriv
  1 - Disallow cpu events for unpriv
  2 - Disallow kernel profiling for unpriv
```

Easiest fix is to set this to `0` or `-1`. This crate currently doesn't expose any raw tracepoint access, so 0 should be fine.

On Arch: https://wiki.archlinux.org/index.php/sysctl#Configuration.

`echo 'kernel.perf_event_paranoid=0' | sudo tee /etc/sysctl.d/perf-event-permissive.conf`

# Building

Currently relies on bindgen's buildscript setup, which depends on libclang.
