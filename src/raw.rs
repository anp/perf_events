#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(unused)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

use std::fmt::{Debug, Formatter, Result as FmtResult};

impl Debug for perf_event_attr {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        unsafe {
            f.debug_struct("perf_event_attr")
                .field("type_", &self.type_)
                .field("size", &self.size)
                .field("config", &self.config)
                .field("sample_type", &self.sample_type)
                .field("read_format", &self.read_format)
                .field("bp_type", &self.bp_type)
                .field("branch_sample_type", &self.branch_sample_type)
                .field("sample_regs_user", &self.sample_regs_user)
                .field("sample_stack_user", &self.sample_stack_user)
                .field("clockid", &self.clockid)
                .field("sample_regs_intr", &self.sample_regs_intr)
                .field("aux_watermark", &self.aux_watermark)
                .field("sample_max_stack", &self.sample_max_stack)
                .field("__reserved_2", &self.__reserved_2)
                .field(
                    "sample_period_or_freq",
                    &self.__bindgen_anon_1.sample_period,
                )
                .field(
                    "wakeup_events_or_watermark",
                    &self.__bindgen_anon_2.wakeup_events,
                )
                .field("__bindgen_anon_3", &self.__bindgen_anon_3.bp_addr)
                .field("__bindgen_anon_4", &self.__bindgen_anon_4.bp_len)
                .field("disabled", &self.disabled())
                .field("inherit", &self.inherit())
                .field("pinned", &self.pinned())
                .field("exclusive", &self.exclusive())
                .field("exclude_user", &self.exclude_user())
                .field("exclude_kernel", &self.exclude_kernel())
                .field("exclude_hv", &self.exclude_hv())
                .field("exclude_idle", &self.exclude_idle())
                .field("mmap", &self.mmap())
                .field("comm", &self.comm())
                .field("freq", &self.freq())
                .field("inherit_stat", &self.inherit_stat())
                .field("enable_on_exec", &self.enable_on_exec())
                .field("task", &self.task())
                .field("watermark", &self.watermark())
                .field("precise_ip", &self.precise_ip())
                .field("mmap_data", &self.mmap_data())
                .field("sample_id_all", &self.sample_id_all())
                .field("exclude_host", &self.exclude_host())
                .field("exclude_guest", &self.exclude_guest())
                .field("exclude_callchain_kernel", &self.exclude_callchain_kernel())
                .field("exclude_callchain_user", &self.exclude_callchain_user())
                .field("mmap2", &self.mmap2())
                .field("comm_exec", &self.comm_exec())
                .field("use_clockid", &self.use_clockid())
                .field("context_switch", &self.context_switch())
                .field("write_backward", &self.write_backward())
                .field("namespaces", &self.namespaces())
                .finish()
        }
    }
}

impl PartialEq for perf_event_attr {
    fn eq(&self, other: &Self) -> bool {
        macro_rules! check {
            ($e:ident. $f:ident) => {
                if self.$e.$f != other.$e.$f {
                    return false;
                }
            };
            ($e:ident) => {
                if self.$e != other.$e {
                    return false;
                }
            };
        }

        check!(type_);
        check!(size);
        check!(config);
        check!(sample_type);
        check!(read_format);
        check!(_bitfield_1);
        check!(bp_type);
        check!(branch_sample_type);
        check!(sample_regs_user);
        check!(sample_stack_user);
        check!(clockid);
        check!(sample_regs_intr);
        check!(aux_watermark);
        check!(sample_max_stack);
        check!(__reserved_2);

        unsafe {
            check!(__bindgen_anon_1.sample_period);
            check!(__bindgen_anon_2.wakeup_events);
            check!(__bindgen_anon_3.bp_addr);
            check!(__bindgen_anon_4.bp_len);
        }

        true
    }
}
