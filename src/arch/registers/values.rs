use crate::{bits, numeric};

bits! {
    pub type SatpValue: u64 {
        ppn: 0 => 43,
        asid: 44 => 59,
        mode: 60 => 63
    }
}

numeric! {
    pub enum SatpMode: u64 {
        BARE = 0,
        SV39 = 8,
        SV48 = 9,
        SV57 = 10,
        SV64 = 11,
    }
}

bits! {
    pub type SStatusBits: u64 {
        spp: 8,
        spie: 5,
        sie: 1
    }
}
