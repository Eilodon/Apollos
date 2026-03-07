#![allow(clippy::large_enum_variant)]
//! Shared message contracts for Apollos client/server communication.

pub mod contracts;
pub mod transport;

pub mod pb {
    #![allow(clippy::derive_partial_eq_without_eq)]

    pub mod apollos {
        pub mod messages {
            pub mod v1 {
                include!(concat!(env!("OUT_DIR"), "/apollos.messages.v1.rs"));
            }
        }

        pub mod types {
            pub mod v1 {
                include!(concat!(env!("OUT_DIR"), "/apollos.types.v1.rs"));
            }
        }
    }

    pub use apollos::messages::v1 as messages_v1;
    pub use apollos::types::v1 as types_v1;
}
