use memmap::MmapOptions;
use nova_vm::{
    ecmascript::{
        builtins::{ArgumentsList, Behaviour, BuiltinFunctionArgs, create_builtin_function},
        execution::{
            Agent, DefaultHostHooks, JsResult,
            agent::{GcAgent, Options},
        },
        types::{
            InternalMethods, IntoValue, Object, PropertyDescriptor, PropertyKey,
            String as JsString, Value,
        },
    },
    engine::context::{Bindable, GcScope},
};
use std::{
    fs::File,
    io::{Write, stderr, stdout},
    os::fd::{FromRawFd, RawFd},
};

const REPRL_CRFD: i32 = 100;
const REPRL_CWFD: i32 = 101;
const REPRL_DRFD: RawFd = 102;
const REPRL_DWFD: RawFd = 103;

unsafe extern "C" {
    fn __sanitizer_cov_reset_edgeguards();
}

fn initialize_global_object_with_fuzzilli(agent: &mut Agent, global: Object, mut gc: GcScope) {
    fn fuzzilli<'gc>(
        agent: &mut Agent,
        _: Value,
        args: ArgumentsList,
        gc: GcScope<'gc, '_>,
    ) -> JsResult<'gc, Value<'gc>> {
        let args = args.bind(gc.nogc());
        let Value::String(cmd) = args.get(0) else {
            panic!("first arg must be a string");
        };
        let cmd = cmd.as_str(agent).expect("first arg not a str");
        match cmd {
            "FUZZILLI_PRINT" => {
                let mut dw_file = unsafe { File::from_raw_fd(REPRL_DWFD) };
                let Value::String(str) = args.get(1) else {
                    panic!("second arg must be a string")
                };
                let buf = str.as_str(agent).expect("print argument empty").as_bytes();
                dw_file
                    .write(buf)
                    .expect("can't write out put FUZZILLI_PRINT");
                dw_file.flush().expect("can't flush output FUZZILLI_PRINT");
                JsResult::Ok(Value::Null)
            }
            "FUZZILLI_CRASH" => {
                let Value::Integer(arg) = args.get(1) else {
                    panic!("second fuzzilli crash arg is an int")
                };
                let arg = arg.into_i64();
                match arg {
                    0 => unsafe {
                        // Explicitly write out of bounds to crash
                        let ptr = 0x41414141 as *mut usize;
                        let val = 0x1337 as usize;
                        std::ptr::write(ptr, val);
                        JsResult::Ok(Value::Null)
                    },
                    _ => panic!("{}", arg),
                }
            }
            _ => panic!("unknown command"),
        }
    }

    let function = create_builtin_function(
        agent,
        Behaviour::Regular(fuzzilli),
        BuiltinFunctionArgs::new(2, "fuzzilli"),
        gc.nogc(),
    );

    let property_key = PropertyKey::from_static_str(agent, "fuzzilli", gc.nogc());

    global
        .internal_define_own_property(
            agent,
            property_key.unbind(),
            PropertyDescriptor {
                value: Some(function.into_value().unbind()),
                writable: Some(true),
                enumerable: Some(false),
                configurable: Some(true),
                ..Default::default()
            },
            gc.reborrow(),
        )
        .unwrap();
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

        let create_global_object: Option<for<'a> fn(&mut Agent, GcScope<'a, '_>) -> Object<'a>> =
            None;
        let create_global_this_value: Option<
            for<'a> fn(&mut Agent, GcScope<'a, '_>) -> Object<'a>,
        > = None;
        let initialize_global: Option<fn(&mut Agent, Object, GcScope)> =
            Some(initialize_global_object_with_fuzzilli);
        let realm = agent.create_realm(
            create_global_object,
            create_global_this_value,
            initialize_global,
        );
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
