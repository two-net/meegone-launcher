//! [`NetworkAdapter`] on Android: the network's state, whether Data Saver is on (PWR-04,
//! ADR 0029), and HTTPS requests (SEC-01, SEC-02, PRIV-02, ADR 0027).
//!
//! Questions go to the Java `Bridge` object, whose method names and signatures must stay in
//! step with the ones looked up here. `Bridge.httpsGet` makes one request at a time with the
//! platform's certificate validation, and never follows a redirect itself; [`https::fetch`]
//! decides which to follow.
//!
//! `get` blocks, so it must run on a JVM thread other than the main one. Android refuses network
//! access on the main thread, and the bridge answers there with a failure, without a request. A
//! thread that Rust starts itself isn't known to the JVM, so a request from it fails too
//! (ADR 0005).

use super::jni::{Env, GlobalRef, JValue, LocalRef, MethodId, Vm};
use crate::adapters::NetworkAdapter;
use crate::https::{self, Hop};
use crate::model::{DataError, NetworkState};

pub struct AndroidNetwork {
    vm: Vm,
    bridge: GlobalRef,
    /// `int networkState()`: 0 offline, 1 metered, 2 unmetered.
    network_state: MethodId,
    /// `boolean savesData()`: Data Saver restricts MeeGone's background data.
    saves_data: MethodId,
    /// `byte[] httpsGet(String url, int maxBytes, String[] outcome)`: the body, or `null`, with
    /// the outcome's kind and detail in `outcome` (see [`https`]).
    https_get: MethodId,
}

impl AndroidNetwork {
    /// Looks up the bridge's methods once, on the thread that started MeeGone.
    pub fn new(env: Env, bridge: &LocalRef) -> Result<Self, DataError> {
        Ok(Self {
            vm: env.vm()?,
            network_state: env.method_id(bridge, c"networkState", c"()I")?,
            saves_data: env.method_id(bridge, c"savesData", c"()Z")?,
            https_get: env.method_id(
                bridge,
                c"httpsGet",
                c"(Ljava/lang/String;I[Ljava/lang/String;)[B",
            )?,
            bridge: env.global_ref(bridge)?,
        })
    }

    /// One request, with no redirect followed.
    fn hop(&self, url: &str, max_bytes: usize) -> Result<Hop, DataError> {
        let env = self.vm.env()?;
        let url = env.new_string(url)?;
        let outcome = env.new_string_array(["", ""].into_iter())?;
        let max_bytes = JValue::int(i32::try_from(max_bytes).unwrap_or(i32::MAX));
        let body = env.call_bytes(
            &self.bridge,
            self.https_get,
            &[JValue::object(&url), max_bytes, JValue::object(&outcome)],
        )?;
        https::hop(body, &env.string_array(&outcome)?)
    }
}

impl NetworkAdapter for AndroidNetwork {
    /// `Offline` if the bridge can't answer, so nothing is attempted.
    fn state(&self) -> NetworkState {
        let state = self
            .vm
            .env()
            .and_then(|env| env.call_int(&self.bridge, self.network_state, &[]));
        match state {
            Ok(2) => NetworkState::Unmetered,
            Ok(1) => NetworkState::Metered,
            _ => NetworkState::Offline,
        }
    }

    /// `ConnectivityManager.getRestrictBackgroundStatus`. True if the bridge can't answer, so
    /// automatic downloads wait for an unmetered network rather than risk the user's data.
    fn saves_data(&self) -> bool {
        self.vm
            .env()
            .and_then(|env| env.call_bool(&self.bridge, self.saves_data, &[]))
            .unwrap_or(true)
    }

    fn get(&self, https_url: &str, max_bytes: usize) -> Result<Vec<u8>, DataError> {
        https::fetch(https_url, max_bytes, |url| self.hop(url, max_bytes))
    }
}
