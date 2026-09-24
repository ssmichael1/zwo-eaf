//! Error type mapping SDK return codes to Rust.

use std::os::raw::c_int;
use zwo_eaf_sys::*;

/// Errors returned by the ZWO EAF SDK, plus a few crate-level variants.
///
/// Each SDK variant corresponds to an `EAF_*` code from `EAF_focuser.h`.
/// [`Unknown`](Error::Unknown) wraps any code not recognised by this crate
/// (for example from a newer SDK).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Focuser index is out of range (no focuser at that index).
    #[error("invalid focuser index")]
    InvalidIndex,
    /// No focuser with this ID is connected.
    #[error("invalid focuser ID")]
    InvalidId,
    /// A parameter value is out of range.
    #[error("invalid value")]
    InvalidValue,
    /// The focuser was physically disconnected.
    #[error("focuser removed")]
    Removed,
    /// The operation is not allowed while the focuser is moving.
    #[error("focuser is moving")]
    Moving,
    /// The focuser is in an error state.
    #[error("focuser is in error state")]
    ErrorState,
    /// Catch-all hardware or parameter error.
    #[error("general error")]
    GeneralError,
    /// The firmware or model does not support this operation.
    #[error("not supported")]
    NotSupported,
    /// The focuser has not been opened.
    #[error("focuser not open")]
    Closed,
    /// Battery temperature is abnormal.
    #[error("battery information unavailable")]
    BatteryInfo,
    /// A supplied buffer or string has an invalid length.
    #[error("invalid length")]
    InvalidLength,

    // --- Bluetooth LE ------------------------------------------------------
    #[error("BLE read failed")]
    BleReadDataFailed,
    #[error("BLE send failed")]
    BleSendDataFailed,
    #[error("BLE connect failed")]
    BleConnectFailed,
    /// BLE is not connected.
    #[error("BLE disconnected")]
    BleDisconnect,
    #[error("BLE pairing failed")]
    BlePairFailed,
    #[error("BLE clear pairing failed")]
    BleClearPairFailed,
    #[error("BLE pairing timed out")]
    BlePairingTimeout,
    #[error("BLE receive timed out")]
    BleReceiveTimeout,
    /// The BLE device is not supported.
    #[error("BLE device does not exist")]
    BleDeviceNotExists,
    #[error("BLE invalid callback")]
    BleInvalidCallback,
    /// A new pairing request was received.
    #[error("BLE new pair request")]
    BleNewPairRequest,
    #[error("BLE data busy")]
    BleDataBusy,
    /// Firmware length sent and received do not match.
    #[error("BLE firmware size check failed")]
    BleCheckSizeFailed,

    // --- Crate-level -------------------------------------------------------
    /// A blocking wait (e.g. [`Focuser::move_to_and_wait`](crate::Focuser::move_to_and_wait))
    /// did not complete within its timeout.
    #[error("timed out waiting for focuser")]
    Timeout,
    /// The SDK returned a null pointer where a string was expected.
    #[error("SDK returned a null string")]
    NullString,
    /// A [`Focuser`](crate::Focuser) with this ID is already open in this
    /// process. The SDK keeps one connection per ID, so a second handle
    /// would be closed from under it when the first is dropped.
    #[error("focuser is already open")]
    AlreadyOpen,
    /// A string passed to the SDK (e.g. a Bluetooth device name or address)
    /// contains an interior NUL byte and cannot be converted to a C string.
    #[error("string contains an interior NUL byte")]
    InteriorNul,
    /// An error code not mapped by this crate (possibly from a newer SDK).
    #[error("unknown error code: {0}")]
    Unknown(i32),
}

/// Convenience alias used throughout the `zwo-eaf` crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Maps a raw SDK return code to [`Result`].
pub(crate) fn check(code: c_int) -> Result<()> {
    match code {
        EAF_SUCCESS => Ok(()),
        EAF_ERROR_INVALID_INDEX => Err(Error::InvalidIndex),
        EAF_ERROR_INVALID_ID => Err(Error::InvalidId),
        EAF_ERROR_INVALID_VALUE => Err(Error::InvalidValue),
        EAF_ERROR_REMOVED => Err(Error::Removed),
        EAF_ERROR_MOVING => Err(Error::Moving),
        EAF_ERROR_ERROR_STATE => Err(Error::ErrorState),
        EAF_ERROR_GENERAL_ERROR => Err(Error::GeneralError),
        EAF_ERROR_NOT_SUPPORTED => Err(Error::NotSupported),
        EAF_ERROR_CLOSED => Err(Error::Closed),
        EAF_ERROR_BATTER_INFO => Err(Error::BatteryInfo),
        EAF_ERROR_INVALID_LENGTH => Err(Error::InvalidLength),
        EAF_BLE_READ_DATA_FAILED => Err(Error::BleReadDataFailed),
        EAF_BLE_SEND_DATA_FAILED => Err(Error::BleSendDataFailed),
        EAF_BLE_CONNECT_FAILED => Err(Error::BleConnectFailed),
        EAF_BLE_DISCONNECT => Err(Error::BleDisconnect),
        EAF_BLE_PAIR_FAILED => Err(Error::BlePairFailed),
        EAF_BLE_CLEAR_PAIR_FAILED => Err(Error::BleClearPairFailed),
        EAF_BLE_PAIRING_TIMEOUT => Err(Error::BlePairingTimeout),
        EAF_BLE_RECEIVE_TIMEOUT => Err(Error::BleReceiveTimeout),
        EAF_BLE_DEVICE_NOT_EXISTS => Err(Error::BleDeviceNotExists),
        EAF_BLE_INVALID_CALLBACK => Err(Error::BleInvalidCallback),
        EAF_BLE_NEW_PAIR_REQUEST => Err(Error::BleNewPairRequest),
        EAF_BLE_DATA_BUSY => Err(Error::BleDataBusy),
        EAF_BLE_CHECK_SIZE_FAILED => Err(Error::BleCheckSizeFailed),
        other => Err(Error::Unknown(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_maps_to_ok() {
        assert_eq!(check(EAF_SUCCESS), Ok(()));
    }

    #[test]
    fn every_usb_code_maps() {
        let cases = [
            (EAF_ERROR_INVALID_INDEX, Error::InvalidIndex),
            (EAF_ERROR_INVALID_ID, Error::InvalidId),
            (EAF_ERROR_INVALID_VALUE, Error::InvalidValue),
            (EAF_ERROR_REMOVED, Error::Removed),
            (EAF_ERROR_MOVING, Error::Moving),
            (EAF_ERROR_ERROR_STATE, Error::ErrorState),
            (EAF_ERROR_GENERAL_ERROR, Error::GeneralError),
            (EAF_ERROR_NOT_SUPPORTED, Error::NotSupported),
            (EAF_ERROR_CLOSED, Error::Closed),
            (EAF_ERROR_BATTER_INFO, Error::BatteryInfo),
            (EAF_ERROR_INVALID_LENGTH, Error::InvalidLength),
        ];
        for (code, err) in cases {
            assert_eq!(check(code), Err(err), "code {code}");
        }
    }

    #[test]
    fn every_ble_code_maps() {
        let cases = [
            (EAF_BLE_READ_DATA_FAILED, Error::BleReadDataFailed),
            (EAF_BLE_SEND_DATA_FAILED, Error::BleSendDataFailed),
            (EAF_BLE_CONNECT_FAILED, Error::BleConnectFailed),
            (EAF_BLE_DISCONNECT, Error::BleDisconnect),
            (EAF_BLE_PAIR_FAILED, Error::BlePairFailed),
            (EAF_BLE_CLEAR_PAIR_FAILED, Error::BleClearPairFailed),
            (EAF_BLE_PAIRING_TIMEOUT, Error::BlePairingTimeout),
            (EAF_BLE_RECEIVE_TIMEOUT, Error::BleReceiveTimeout),
            (EAF_BLE_DEVICE_NOT_EXISTS, Error::BleDeviceNotExists),
            (EAF_BLE_INVALID_CALLBACK, Error::BleInvalidCallback),
            (EAF_BLE_NEW_PAIR_REQUEST, Error::BleNewPairRequest),
            (EAF_BLE_DATA_BUSY, Error::BleDataBusy),
            (EAF_BLE_CHECK_SIZE_FAILED, Error::BleCheckSizeFailed),
        ];
        for (code, err) in cases {
            assert_eq!(check(code), Err(err), "code {code}");
        }
    }

    #[test]
    fn unknown_codes_are_preserved() {
        assert_eq!(check(12), Err(Error::Unknown(12)));
        assert_eq!(check(63), Err(Error::Unknown(63)));
        assert_eq!(check(EAF_ERROR_END), Err(Error::Unknown(-1)));
    }

    #[test]
    fn display_is_human_readable() {
        assert_eq!(Error::Moving.to_string(), "focuser is moving");
        assert_eq!(Error::Unknown(99).to_string(), "unknown error code: 99");
        assert_eq!(Error::AlreadyOpen.to_string(), "focuser is already open");
    }
}
