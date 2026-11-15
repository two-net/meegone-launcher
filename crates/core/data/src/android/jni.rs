//! A minimal, hand-written binding to the Java Native Interface (ADR 0005).
//!
//! Only the functions MeeGone uses are bound. Each is read from the JNI function tables by its
//! slot, which the JNI specification fixes; the slots below were checked against `jni.h` of
//! NDK 27.3. Everything this file exposes is safe to call, so the rest of the app needs no
//! `unsafe` (arch-check allows it only here and in the entry points).
//!
//! After every call into Java, a pending Java exception is described to logcat, cleared and
//! returned as [`DataError::Failed`]. Calling JNI with an exception pending is an error that
//! CheckJNI, on in debug builds, turns into an abort.

use std::ffi::{CStr, CString, c_char, c_void};
use std::{mem, ptr, slice};

use crate::model::DataError;

/// A Java object reference: `jobject`, `jclass` or `jstring`.
pub type JObject = *mut c_void;
/// `JNIEnv*` and `JavaVM*`: each points to a pointer to a table of functions.
pub type RawEnv = *mut *const *const c_void;
type RawVm = *mut *const *const c_void;
type JMethodId = *mut c_void;
type JBoolean = u8;
type JInt = i32;
type JLong = i64;

const JNI_OK: JInt = 0;
const JNI_VERSION_1_6: JInt = 0x0001_0006;

/// Slots in the `JNIEnv` function table (`JNINativeInterface`).
mod env_slot {
    pub const FIND_CLASS: usize = 6;
    pub const THROW_NEW: usize = 14;
    pub const EXCEPTION_DESCRIBE: usize = 16;
    pub const EXCEPTION_CLEAR: usize = 17;
    pub const NEW_GLOBAL_REF: usize = 21;
    pub const DELETE_GLOBAL_REF: usize = 22;
    pub const DELETE_LOCAL_REF: usize = 23;
    pub const GET_OBJECT_CLASS: usize = 31;
    pub const GET_METHOD_ID: usize = 33;
    pub const CALL_OBJECT_METHOD_A: usize = 36;
    pub const CALL_BOOLEAN_METHOD_A: usize = 39;
    pub const CALL_LONG_METHOD_A: usize = 54;
    pub const GET_STRING_LENGTH: usize = 164;
    pub const GET_STRING_CHARS: usize = 165;
    pub const RELEASE_STRING_CHARS: usize = 166;
    pub const GET_JAVA_VM: usize = 219;
    pub const EXCEPTION_CHECK: usize = 228;
}

/// Slots in the `JavaVM` function table (`JNIInvokeInterface`).
mod vm_slot {
    pub const GET_ENV: usize = 6;
}

/// One argument of a `Call…MethodA` call: a `jvalue`. Only `long` arguments are used so far.
#[repr(C)]
#[derive(Clone, Copy)]
pub union JValue {
    long: JLong,
}

impl JValue {
    pub fn long(value: i64) -> Self {
        Self { long: value }
    }
}

/// The calling thread's JNI environment. It is valid only on that thread, during the native
/// call that received it, so it is never stored; [`Vm::env`] gets it again later.
#[derive(Clone, Copy)]
pub struct Env(RawEnv);

/// A local reference the JVM passed to a native method. Valid only during that call.
pub struct LocalRef(JObject);

impl LocalRef {
    /// # Safety
    ///
    /// `raw` is a reference the JVM passed to the native method that is running now, and the
    /// result is dropped before that method returns.
    pub unsafe fn from_raw(raw: JObject) -> Self {
        Self(raw)
    }
}

/// A reference that stays valid on any thread until it is dropped.
pub struct GlobalRef {
    vm: Vm,
    raw: JObject,
}

// SAFETY: a global reference may be used from any thread until it is deleted (JNI
// specification, "Global and Local References"), and only `Drop` deletes it.
unsafe impl Send for GlobalRef {}
// SAFETY: as for `Send`; JNI calls on a global reference need no outside locking.
unsafe impl Sync for GlobalRef {}

impl Drop for GlobalRef {
    fn drop(&mut self) {
        // Off a JVM thread the reference leaks, which is harmless at process exit.
        if let Ok(env) = self.vm.env() {
            type DeleteGlobalRef = unsafe extern "system" fn(RawEnv, JObject);
            // SAFETY: `raw` came from `NewGlobalRef` and is deleted only here.
            unsafe {
                env.function::<DeleteGlobalRef>(env_slot::DELETE_GLOBAL_REF)(env.0, self.raw)
            };
        }
    }
}

/// A method found with [`Env::method_id`].
#[derive(Clone, Copy)]
pub struct MethodId(JMethodId);

// SAFETY: method IDs stay valid while their class is loaded, on every thread (JNI
// specification, "Accessing Methods"). MeeGone's classes live as long as its process.
unsafe impl Send for MethodId {}
// SAFETY: as for `Send`.
unsafe impl Sync for MethodId {}

/// The Java VM, which outlives every thread and is valid process-wide.
#[derive(Clone, Copy)]
pub struct Vm(RawVm);

// SAFETY: a `JavaVM*` is valid for the life of the process, and its functions may be called
// from any thread (JNI specification, "Invocation API").
unsafe impl Send for Vm {}
// SAFETY: as for `Send`.
unsafe impl Sync for Vm {}

impl Vm {
    /// The current thread's environment.
    ///
    /// Fails on a thread the JVM doesn't know. Every call comes from the main thread today;
    /// attaching other threads arrives with background work (ADR 0005).
    pub fn env(&self) -> Result<Env, DataError> {
        type GetEnv = unsafe extern "system" fn(RawVm, *mut RawEnv, JInt) -> JInt;
        let mut env: RawEnv = ptr::null_mut();
        // SAFETY: `self.0` is a valid `JavaVM*`, and `GET_ENV` is that table's `GetEnv`.
        let status = unsafe {
            let get_env: GetEnv = mem::transmute_copy(&*(*self.0).add(vm_slot::GET_ENV));
            get_env(self.0, &mut env, JNI_VERSION_1_6)
        };
        if status == JNI_OK && !env.is_null() {
            Ok(Env(env))
        } else {
            Err(DataError::Failed(
                "This thread can't reach the system".into(),
            ))
        }
    }
}

impl Env {
    /// # Safety
    ///
    /// `raw` is the `JNIEnv*` the JVM passed to the native method that is running now on this
    /// thread.
    pub unsafe fn from_raw(raw: RawEnv) -> Self {
        Self(raw)
    }

    /// The function in `slot` of the `JNIEnv` table, as `F`.
    ///
    /// # Safety
    ///
    /// `F` is the exact `extern "system"` signature of the function in that slot.
    unsafe fn function<F: Copy>(&self, slot: usize) -> F {
        debug_assert_eq!(mem::size_of::<F>(), mem::size_of::<*const c_void>());
        // SAFETY: `self.0` points to the JVM's table, which has 233 slots, more than any used
        // here; the caller vouches for `F`.
        unsafe { mem::transmute_copy(&*(*self.0).add(slot)) }
    }

    /// Fails if Java threw, after describing the exception to logcat and clearing it.
    fn check(&self) -> Result<(), DataError> {
        type ExceptionCheck = unsafe extern "system" fn(RawEnv) -> JBoolean;
        type NoArgs = unsafe extern "system" fn(RawEnv);
        // SAFETY: the signatures match `jni.h`, and these three may be called while an
        // exception is pending.
        unsafe {
            if self.function::<ExceptionCheck>(env_slot::EXCEPTION_CHECK)(self.0) == 0 {
                return Ok(());
            }
            self.function::<NoArgs>(env_slot::EXCEPTION_DESCRIBE)(self.0);
            self.function::<NoArgs>(env_slot::EXCEPTION_CLEAR)(self.0);
        }
        Err(DataError::Failed("The system couldn't do that".into()))
    }

    fn delete_local(&self, raw: JObject) {
        if raw.is_null() {
            return;
        }
        type DeleteLocalRef = unsafe extern "system" fn(RawEnv, JObject);
        // SAFETY: `raw` is a live local reference this file created and no longer uses.
        unsafe { self.function::<DeleteLocalRef>(env_slot::DELETE_LOCAL_REF)(self.0, raw) };
    }

    /// The JVM, for use on later calls and other threads.
    pub fn vm(&self) -> Result<Vm, DataError> {
        type GetJavaVm = unsafe extern "system" fn(RawEnv, *mut RawVm) -> JInt;
        let mut vm: RawVm = ptr::null_mut();
        // SAFETY: the signature matches `jni.h`, and `vm` is a valid place to write to.
        let status = unsafe { self.function::<GetJavaVm>(env_slot::GET_JAVA_VM)(self.0, &mut vm) };
        self.check()?;
        if status == JNI_OK && !vm.is_null() {
            Ok(Vm(vm))
        } else {
            Err(DataError::Failed("The system couldn't do that".into()))
        }
    }

    /// Keeps `object` alive beyond the current native call.
    pub fn global_ref(&self, object: &LocalRef) -> Result<GlobalRef, DataError> {
        type NewGlobalRef = unsafe extern "system" fn(RawEnv, JObject) -> JObject;
        let vm = self.vm()?;
        // SAFETY: the signature matches `jni.h`, and `object` is a live local reference.
        let raw =
            unsafe { self.function::<NewGlobalRef>(env_slot::NEW_GLOBAL_REF)(self.0, object.0) };
        self.check()?;
        if raw.is_null() {
            return Err(DataError::Failed("The system couldn't do that".into()));
        }
        Ok(GlobalRef { vm, raw })
    }

    /// The instance method `name` with JNI signature `signature` of `object`'s class.
    pub fn method_id(
        &self,
        object: &LocalRef,
        name: &CStr,
        signature: &CStr,
    ) -> Result<MethodId, DataError> {
        type GetObjectClass = unsafe extern "system" fn(RawEnv, JObject) -> JObject;
        type GetMethodId =
            unsafe extern "system" fn(RawEnv, JObject, *const c_char, *const c_char) -> JMethodId;
        // SAFETY: the signatures match `jni.h`; `object` is a live local reference, and the
        // class reference is deleted once the ID is found.
        let id = unsafe {
            let class =
                self.function::<GetObjectClass>(env_slot::GET_OBJECT_CLASS)(self.0, object.0);
            self.check()?;
            let id = self.function::<GetMethodId>(env_slot::GET_METHOD_ID)(
                self.0,
                class,
                name.as_ptr(),
                signature.as_ptr(),
            );
            let found = self.check();
            self.delete_local(class);
            found?;
            id
        };
        if id.is_null() {
            return Err(DataError::Failed("The system couldn't do that".into()));
        }
        Ok(MethodId(id))
    }

    /// Calls a method returning `boolean`.
    pub fn call_bool(
        &self,
        object: &GlobalRef,
        method: MethodId,
        args: &[JValue],
    ) -> Result<bool, DataError> {
        type CallBooleanMethodA =
            unsafe extern "system" fn(RawEnv, JObject, JMethodId, *const JValue) -> JBoolean;
        // SAFETY: the signature matches `jni.h`; the caller found `method` on this object's
        // class with a `boolean` return type and passes its arguments.
        let result = unsafe {
            self.function::<CallBooleanMethodA>(env_slot::CALL_BOOLEAN_METHOD_A)(
                self.0,
                object.raw,
                method.0,
                args.as_ptr(),
            )
        };
        self.check()?;
        Ok(result != 0)
    }

    /// Calls a method returning `long`.
    pub fn call_long(
        &self,
        object: &GlobalRef,
        method: MethodId,
        args: &[JValue],
    ) -> Result<i64, DataError> {
        type CallLongMethodA =
            unsafe extern "system" fn(RawEnv, JObject, JMethodId, *const JValue) -> JLong;
        // SAFETY: as for `call_bool`, for a `long` return type.
        let result = unsafe {
            self.function::<CallLongMethodA>(env_slot::CALL_LONG_METHOD_A)(
                self.0,
                object.raw,
                method.0,
                args.as_ptr(),
            )
        };
        self.check()?;
        Ok(result)
    }

    /// Calls a method returning `String`, which may be `null`.
    pub fn call_string(
        &self,
        object: &GlobalRef,
        method: MethodId,
        args: &[JValue],
    ) -> Result<Option<String>, DataError> {
        type CallObjectMethodA =
            unsafe extern "system" fn(RawEnv, JObject, JMethodId, *const JValue) -> JObject;
        // SAFETY: as for `call_bool`, for a `String` return type.
        let result = unsafe {
            self.function::<CallObjectMethodA>(env_slot::CALL_OBJECT_METHOD_A)(
                self.0,
                object.raw,
                method.0,
                args.as_ptr(),
            )
        };
        self.check()?;
        if result.is_null() {
            return Ok(None);
        }
        let text = self.string_of(result);
        self.delete_local(result);
        text.map(Some)
    }

    /// A Java string passed to a native method.
    pub fn string(&self, string: &LocalRef) -> Result<String, DataError> {
        self.string_of(string.0)
    }

    /// Reads a live, non-null `jstring` as UTF-16, so every character survives.
    fn string_of(&self, string: JObject) -> Result<String, DataError> {
        type GetStringLength = unsafe extern "system" fn(RawEnv, JObject) -> JInt;
        type GetStringChars =
            unsafe extern "system" fn(RawEnv, JObject, *mut JBoolean) -> *const u16;
        type ReleaseStringChars = unsafe extern "system" fn(RawEnv, JObject, *const u16);
        // SAFETY: the signatures match `jni.h`; `string` is a live `jstring`, its characters
        // are read only between `GetStringChars` and `ReleaseStringChars`, and they number
        // `GetStringLength`.
        unsafe {
            let length =
                self.function::<GetStringLength>(env_slot::GET_STRING_LENGTH)(self.0, string);
            self.check()?;
            let chars = self.function::<GetStringChars>(env_slot::GET_STRING_CHARS)(
                self.0,
                string,
                ptr::null_mut(),
            );
            self.check()?;
            if chars.is_null() {
                return Err(DataError::Failed("The system couldn't do that".into()));
            }
            let length = usize::try_from(length).unwrap_or(0);
            let text = String::from_utf16_lossy(slice::from_raw_parts(chars, length));
            self.function::<ReleaseStringChars>(env_slot::RELEASE_STRING_CHARS)(
                self.0, string, chars,
            );
            Ok(text)
        }
    }

    /// Throws a `java.lang.RuntimeException` with `message` when the native call returns, so a
    /// bug in MeeGone reaches logcat instead of vanishing. Keeps an exception already pending.
    pub fn throw_runtime_exception(&self, message: &str) {
        type ExceptionCheck = unsafe extern "system" fn(RawEnv) -> JBoolean;
        type FindClass = unsafe extern "system" fn(RawEnv, *const c_char) -> JObject;
        type ThrowNew = unsafe extern "system" fn(RawEnv, JObject, *const c_char) -> JInt;
        // JNI strings are Modified UTF-8; plain ASCII is the same in both.
        let ascii: String = message
            .chars()
            .map(|c| if c.is_ascii() && c != '\0' { c } else { '?' })
            .collect();
        let message = CString::new(ascii).unwrap_or_default();
        // SAFETY: the signatures match `jni.h`; the class reference is deleted after use.
        unsafe {
            if self.function::<ExceptionCheck>(env_slot::EXCEPTION_CHECK)(self.0) != 0 {
                return;
            }
            let class = self.function::<FindClass>(env_slot::FIND_CLASS)(
                self.0,
                c"java/lang/RuntimeException".as_ptr(),
            );
            if class.is_null() {
                return;
            }
            self.function::<ThrowNew>(env_slot::THROW_NEW)(self.0, class, message.as_ptr());
            self.delete_local(class);
        }
    }
}
