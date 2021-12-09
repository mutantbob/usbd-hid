//! Implements HID functionality for a usb-device device.
use usb_device::class_prelude::*;
use usb_device::Result;

use crate::descriptor::AsInputReport;
extern crate ssmarshal;
use ssmarshal::serialize;

const USB_CLASS_HID: u8 = 0x03;
const USB_SUBCLASS_NONE: u8 = 0x00;
const USB_PROTOCOL_NONE: u8 = 0x00;

// HID
const HID_DESC_DESCTYPE_HID: u8 = 0x21;
const HID_DESC_DESCTYPE_HID_REPORT: u8 = 0x22;
const HID_DESC_SPEC_1_10: [u8; 2] = [0x10, 0x01];
const HID_DESC_COUNTRY_UNSPEC: u8 = 0x00;

const HID_REQ_SET_IDLE: u8 = 0x0a;
const HID_REQ_GET_IDLE: u8 = 0x02;
const HID_REQ_GET_REPORT: u8 = 0x01;
const HID_REQ_SET_REPORT: u8 = 0x09;

/// HIDClass provides an interface to declare, read & write HID reports.
///
/// Users are expected to provide the report descriptor, as well as pack
/// and unpack reports which are read or staged for transmission.
pub struct HIDClass<'a, B: UsbBus> {
    if_num: InterfaceNumber,
    out_ep: Option<EndpointOut<'a, B>>,
    in_ep: Option<EndpointIn<'a, B>>,
    report_descriptor: &'static [u8],
}

impl<B: UsbBus> HIDClass<'_, B> {
    /// Creates a new HIDClass with the provided UsbBus & HID report descriptor.
    ///
    /// poll_ms configures how frequently the host should poll for reading/writing
    /// HID reports. A lower value means better throughput & latency, at the expense
    /// of CPU on the device & bandwidth on the bus. A value of 10 is reasonable for
    /// high performance uses, and a value of 255 is good for best-effort usecases.
    ///
    /// This allocates two endpoints (IN and OUT).
    /// See new_ep_in (IN endpoint only) and new_ep_out (OUT endpoint only) to only create a single
    /// endpoint.
    pub fn new<'a>(
        alloc: &'a UsbBusAllocator<B>,
        report_descriptor: &'static [u8],
        poll_ms: u8,
    ) -> HIDClass<'a, B> {
        HIDClass {
            if_num: alloc.interface(),
            out_ep: Some(alloc.interrupt(64, poll_ms)),
            in_ep: Some(alloc.interrupt(64, poll_ms)),
            report_descriptor,
        }
    }

    /// Creates a new HIDClass with the provided UsbBus & HID report descriptor.
    /// See new() for more details.
    pub fn new_ep_in<'a>(
        alloc: &'a UsbBusAllocator<B>,
        report_descriptor: &'static [u8],
        poll_ms: u8,
    ) -> HIDClass<'a, B> {
        HIDClass {
            if_num: alloc.interface(),
            out_ep: None,
            in_ep: Some(alloc.interrupt(64, poll_ms)),
            report_descriptor,
        }
    }

    /// Creates a new HIDClass with the provided UsbBus & HID report descriptor.
    /// See new() for more details.
    pub fn new_ep_out<'a>(
        alloc: &'a UsbBusAllocator<B>,
        report_descriptor: &'static [u8],
        poll_ms: u8,
    ) -> HIDClass<'a, B> {
        HIDClass {
            if_num: alloc.interface(),
            out_ep: Some(alloc.interrupt(64, poll_ms)),
            in_ep: None,
            report_descriptor,
        }
    }

    /// Tries to write an input report by serializing the given report structure.
    /// A BufferOverflow error is returned if the serialized report is greater than
    /// 64 bytes in size.
    pub fn push_input<IR: AsInputReport>(&self, r: &IR) -> Result<usize> {
        if let Some(ep) = &self.in_ep {
            let mut buff: [u8; 64] = [0; 64];
            let size = match serialize(&mut buff, r) {
                Ok(l) => l,
                Err(_) => return Err(UsbError::BufferOverflow),
            };
            ep.write(&buff[0..size])
        } else {
            Err(UsbError::InvalidEndpoint)
        }
    }

    pub fn maybe_push_input<'a, IR: AsInputReport>(
        &self,
         mut producer: impl FnMut() -> IR,
    ) -> Option<Result<usize>> {
        if let Some(ep) = &self.in_ep {
            let mut buff: [u8; 64] = [0; 64];
            ep.maybe_write(|| {
                let size = match serialize(&mut buff, &producer()) {
                    Ok(l) => l,
                    Err(_) => return Err(UsbError::BufferOverflow),
                };
                Ok(&buff[0..size])
            })
        } else {
            Some(Err(UsbError::InvalidEndpoint))
        }
    }

    /// Tries to write an input (device-to-host) report from the given raw bytes.
    /// Data is expected to be a valid HID report for INPUT items. If report ID's
    /// were used in the descriptor, the report ID corresponding to this report
    /// must be be present before the contents of the report.
    pub fn push_raw_input(&self, data: &[u8]) -> Result<usize> {
        if let Some(ep) = &self.in_ep {
            ep.write(data)
        } else {
            Err(UsbError::InvalidEndpoint)
        }
    }

    /// Tries to read an output (host-to-device) report as raw bytes. Data
    /// is expected to be sized appropriately to contain any valid HID report
    /// for OUTPUT items, including the report ID prefix if report IDs are used.
    pub fn pull_raw_output(&self, data: &mut [u8]) -> Result<usize> {
        if let Some(ep) = &self.out_ep {
            ep.read(data)
        } else {
            Err(UsbError::InvalidEndpoint)
        }
    }
}

impl<B: UsbBus> UsbClass<B> for HIDClass<'_, B> {
    fn get_configuration_descriptors(&self, writer: &mut DescriptorWriter) -> Result<()> {
        writer.interface(
            self.if_num,
            USB_CLASS_HID,
            USB_SUBCLASS_NONE,
            USB_PROTOCOL_NONE,
        )?;

        // HID descriptor
        writer.write(
            HID_DESC_DESCTYPE_HID,
            &[
                // HID Class spec version
                HID_DESC_SPEC_1_10[0],
                HID_DESC_SPEC_1_10[1],
                // Country code not supported
                HID_DESC_COUNTRY_UNSPEC,
                // Number of following descriptors
                1,
                // We have a HID report descriptor the host should read
                HID_DESC_DESCTYPE_HID_REPORT,
                // HID report descriptor size,
                (self.report_descriptor.len() & 0xFF) as u8,
                (self.report_descriptor.len() >> 8 & 0xFF) as u8,
            ],
        )?;

        if let Some(ep) = &self.out_ep {
            writer.endpoint(ep)?;
        }
        if let Some(ep) = &self.in_ep {
            writer.endpoint(ep)?;
        }
        Ok(())
    }

    // Handle control requests to the host.
    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = xfer.request();

        // Bail out if its not relevant to our interface.
        if req.index != u8::from(self.if_num) as u16 {
            return;
        }

        match (req.request_type, req.request) {
            (control::RequestType::Standard, control::Request::GET_DESCRIPTOR) => {
                match (req.value >> 8) as u8 {
                    HID_DESC_DESCTYPE_HID_REPORT => {
                        xfer.accept_with_static(self.report_descriptor).ok();
                    }
                    HID_DESC_DESCTYPE_HID => {
                        let buf = &[
                            // Length of buf inclusive of size prefix
                            9,
                            // Descriptor type
                            HID_DESC_DESCTYPE_HID,
                            // HID Class spec version
                            HID_DESC_SPEC_1_10[0],
                            HID_DESC_SPEC_1_10[1],
                            // Country code not supported
                            HID_DESC_COUNTRY_UNSPEC,
                            // Number of following descriptors
                            1,
                            // We have a HID report descriptor the host should read
                            HID_DESC_DESCTYPE_HID_REPORT,
                            // HID report descriptor size,
                            (self.report_descriptor.len() & 0xFF) as u8,
                            (self.report_descriptor.len() >> 8 & 0xFF) as u8,
                        ];
                        xfer.accept_with(buf).ok();
                    }
                    _ => {}
                }
            }
            (control::RequestType::Class, HID_REQ_GET_REPORT) => {
                xfer.reject().ok(); // Not supported for now
            }
            (control::RequestType::Class, HID_REQ_GET_IDLE) => {
                xfer.reject().ok(); // Not supported for now
            }
            _ => {}
        }
    }

    // Handle a control request from the host.
    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = xfer.request();

        // Bail out if its not relevant to our interface.
        if !(req.recipient == control::Recipient::Interface
            && req.index == u8::from(self.if_num) as u16)
        {
            return;
        }

        match req.request {
            HID_REQ_SET_IDLE => {
                xfer.accept().ok();
            }
            HID_REQ_SET_REPORT => {
                xfer.reject().ok(); // Not supported for now
            }
            _ => {
                xfer.reject().ok();
            }
        }
    }
}
