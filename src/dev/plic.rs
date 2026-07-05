// plic@c000000 {
//     phandle = <0x03>;
//     riscv,ndev = <0x5f>;
//     reg = <0x00 0xc000000 0x00 0x600000>;
//     interrupts-extended = <0x02 0xffffffff 0x02 0x09>;
//     interrupt-controller;
//     compatible = "sifive,plic-1.0.0", "riscv,plic0";
//     #address-cells = <0x00>;
//     #interrupt-cells = <0x01>;
// };

use crate::dev::Device;

pub struct Plic {
    device: Device,
}
