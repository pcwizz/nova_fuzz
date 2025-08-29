use memmap::MmapOptions;
use nova_vm::{
    ecmascript::{
        execution::{
            DefaultHostHooks,
            agent::{GcAgent, Options},
        },
        types::{String as JsString, Value},
    },
    engine::context::Bindable,
};
use std::{
    fs::File,
    io::{Write, stderr, stdout},
    os::fd::{FromRawFd, RawFd},
};

const REPRL_CRFD: i32 = 100;
const REPRL_CWFD: i32 = 101;
const REPRL_DRFD: RawFd = 102;
const REPRL_DWFD: i32 = 103;

unsafe extern "C" {
    fn __sanitizer_cov_reset_edgeguards();
}

fn main() {
    let dr_file = unsafe { File::from_raw_fd(REPRL_DRFD) };
    let dr_mmap = unsafe {
        MmapOptions::new()
            .map(&dr_file)
            .expect("couldn't map data read file")
    };

    unsafe {
        libc::write(REPRL_CWFD, b"HELO".as_ptr() as *const libc::c_void, 4);
        let mut buf = [0u8; 4];
        let bytes_read = libc::read(REPRL_CRFD, buf.as_mut_ptr() as *mut libc::c_void, 4);
        assert_eq!(bytes_read, 4);
        assert_eq!(&buf, b"HELO");
    }

    loop {
        unsafe {
            let mut buf = [0u8; 4];
            let bytes_read = libc::read(REPRL_CRFD, buf.as_mut_ptr() as *mut libc::c_void, 4);
            assert_eq!(bytes_read, 4);
            assert_eq!(&buf, b"exec");
        }

        let mut script_len_buf = [0u8; 8];
        unsafe {
            let bytes_read = libc::read(
                REPRL_CRFD,
                script_len_buf.as_mut_ptr() as *mut libc::c_void,
                8,
            );
            assert_eq!(bytes_read, 8);
        }
        let script_len = u64::from_le_bytes(script_len_buf) as usize;

        let script = str::from_utf8(&dr_mmap[..script_len])
            .expect("invalid UTF8")
            .to_string();

        let mut agent = GcAgent::new(
            Options {
                disable_gc: false,
                print_internals: true,
            },
            &DefaultHostHooks,
        );
        let realm = agent.create_default_realm();

        agent.run_in_realm(&realm, |agent, mut gc| {
            let source_text = JsString::from_string(agent, script, gc.nogc());
            let result = agent.run_script(source_text.unbind(), gc.reborrow());

            let status = match result {
                Ok(_) => 0u32,
                Err(e) => match e.value() {
                    Value::Integer(i) => ((i.into_i64() as u32) & 0xFF) << 8,
                    _ => (1u32 & 0xFF) << 8,
                },
            };

            stdout().flush().expect("can't flush stdout");
            stderr().flush().expect("can't flush stderr");

            unsafe {
                libc::write(REPRL_CWFD, &status as *const u32 as *const libc::c_void, 4);
                __sanitizer_cov_reset_edgeguards();
            }
        });

        agent.remove_realm(realm);
    }
}
