// btleplug Source Code File
//
// Copyright 2020 Nonpolynomial Labs LLC. All rights reserved.
//
// Licensed under the BSD 3-Clause license. See LICENSE file in the project root
// for full license information.

use crate::{
    api::{
        AddressType, BDAddr, PeripheralProperties, CommandCallback,
        NotificationHandler, RequestCallback, UUID, Peripheral as ApiPeripheral,
        Characteristic, CentralEvent, EventHandler
    },
    Result, Error
};
use super::{
    adapter::uuid_to_bdaddr,
    internal::{
        CoreBluetoothEvent, CoreBluetoothMessage, CoreBluetoothReplyFuture, CoreBluetoothReply,
    }
};
use std::{
    fmt::{self, Debug, Display, Formatter},
    collections::{BTreeSet},
    iter::FromIterator,
    sync::{
        Arc, Mutex
    }
};
use async_std::{
    sync::{Receiver, Sender},
    task,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Peripheral {
    notification_handlers: Arc<Mutex<Vec<NotificationHandler>>>,
    adapter_handlers: Arc<Mutex<Vec<EventHandler>>>,
    uuid: Uuid,
    characteristics: Arc<Mutex<BTreeSet<Characteristic>>>,
    properties: PeripheralProperties,
    event_receiver: Receiver<CoreBluetoothEvent>,
    message_sender: Sender<CoreBluetoothMessage>,
    // We're not actually holding a peripheral object here, that's held out in
    // the objc thread. We'll just communicate with it through our
    // receiver/sender pair.
}

impl Peripheral {
    pub fn new(uuid: Uuid, local_name: String, adapter_handlers: Arc<Mutex<Vec<EventHandler>>>, event_receiver: Receiver<CoreBluetoothEvent>, message_sender: Sender<CoreBluetoothMessage>) -> Self {
        // Since we're building the object, we have an active advertisement.
        // Build properties now.
        let properties = PeripheralProperties {
            // Rumble required ONLY a BDAddr, not something you can get from
            // MacOS, so we make it up for now. This sucks.
            address: uuid_to_bdaddr(&uuid.to_string()),
            address_type: AddressType::Random,
            local_name: Some(local_name),
            tx_power_level: None,
            manufacturer_data: None,
            discovery_count: 1,
            has_scan_response: true,
        };
        Self {
            properties,
            adapter_handlers,
            characteristics: Arc::new(Mutex::new(BTreeSet::new())),
            notification_handlers: Arc::new(Mutex::new(Vec::new())),
            uuid,
            event_receiver,
            message_sender,
        }
    }

    fn emit(&self, event: CentralEvent) {
        debug!("emitted {:?}", event);
        let handlers = self.adapter_handlers.clone();
        let vec = handlers.lock().unwrap();
        for handler in (*vec).iter() {
            handler(event.clone());
        }
    }
}

impl Display for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // let connected = if self.is_connected() { " connected" } else { "" };
        // let properties = self.properties.lock().unwrap();
        // write!(f, "{} {}{}", self.address, properties.local_name.clone()
        //     .unwrap_or_else(|| "(unknown)".to_string()), connected)
        write!(f, "Peripheral")
    }
}

impl Debug for Peripheral {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        // let connected = if self.is_connected() { " connected" } else { "" };
        // let properties = self.properties.lock().unwrap();
        // let characteristics = self.characteristics.lock().unwrap();
        // write!(f, "{} properties: {:?}, characteristics: {:?} {}", self.address, *properties,
        //        *characteristics, connected)
        write!(f, "Peripheral")
    }
}

fn get_apple_uuid(uuid: UUID) -> Uuid {
    let mut u;
    if let UUID::B128(big_u) = uuid {
        u = big_u;
    } else {
        panic!("Wrong UUID type!");
    }
    u.reverse();
    Uuid::from_bytes(u)
}

impl ApiPeripheral for Peripheral {
    /// Returns the address of the peripheral.
    fn address(&self) -> BDAddr {
        self.properties.address
    }

    /// Returns the set of properties associated with the peripheral. These may be updated over time
    /// as additional advertising reports are received.
    fn properties(&self) -> PeripheralProperties {
        self.properties.clone()
    }

    /// The set of characteristics we've discovered for this device. This will be empty until
    /// `discover_characteristics` or `discover_characteristics_in_range` is called.
    fn characteristics(&self) -> BTreeSet<Characteristic> {
        self.characteristics.lock().unwrap().clone()
    }

    /// Returns true iff we are currently connected to the device.
    fn is_connected(&self) -> bool {
        false
    }

    /// Creates a connection to the device. This is a synchronous operation; if this method returns
    /// Ok there has been successful connection. Note that peripherals allow only one connection at
    /// a time. Operations that attempt to communicate with a device will fail until it is connected.
    fn connect(&self) -> Result<()> {
        info!("Trying device connect!");
        task::block_on(async {
            let fut = CoreBluetoothReplyFuture::default();
            self.message_sender.send(CoreBluetoothMessage::ConnectDevice(self.uuid, fut.get_state_clone())).await;
            match fut.await {
                CoreBluetoothReply::Connected(chars) => {
                    *(self.characteristics.lock().unwrap()) = chars;
                    self.emit(CentralEvent::DeviceConnected(self.properties.address));
                }
                _ => panic!("Shouldn't get anything but connected!"),
            }
        });
        info!("Device connected!");
        Ok(())
    }

    /// Terminates a connection to the device. This is a synchronous operation.
    fn disconnect(&self) -> Result<()> {
        Ok(())
    }

    /// Discovers all characteristics for the device. This is a synchronous operation.
    fn discover_characteristics(&self) -> Result<Vec<Characteristic>> {
        let chrs = self.characteristics.lock().unwrap().clone();
        let v = Vec::from_iter(chrs.into_iter());
        Ok(v)
    }

    /// Discovers characteristics within the specified range of handles. This is a synchronous
    /// operation.
    fn discover_characteristics_in_range(&self, _start: u16, _end: u16) -> Result<Vec<Characteristic>> {
        panic!("NOT IMPLEMENTED");
    }

    /// Sends a command (`write-without-response`) to the characteristic. Takes an optional callback
    /// that will be notified in case of error or when the command has been successfully acked by the
    /// device.
    fn command_async(&self, _characteristic: &Characteristic, _data: &[u8], _handler: Option<CommandCallback>) {
        info!("Trying to command!");
    }

    /// Sends a command (write without response) to the characteristic. Synchronously returns a
    /// `Result` with an error set if the command was not accepted by the device.
    fn command(&self, characteristic: &Characteristic, data: &[u8]) -> Result<()> {
        info!("Trying to command!");
        task::block_on(async {
            let fut = CoreBluetoothReplyFuture::default();
            self.message_sender.send(CoreBluetoothMessage::WriteValue(self.uuid, get_apple_uuid(characteristic.uuid), Vec::from(data), fut.get_state_clone())).await;
            match fut.await {
                CoreBluetoothReply::Ok => {},
                _ => panic!("Didn't subscribe!"),
            }
        });
        Ok(())
    }

    /// Sends a request (write) to the device. Takes an optional callback with either an error if
    /// the request was not accepted or the response from the device.
    fn request_async(&self, _characteristic: &Characteristic,
                     _data: &[u8], _handler: Option<RequestCallback>) {

    }

    /// Sends a request (write) to the device. Synchronously returns either an error if the request
    /// was not accepted or the response from the device.
    fn request(&self, _characteristic: &Characteristic, _data: &[u8]) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    /// Sends a read-by-type request to device for the range of handles covered by the
    /// characteristic and for the specified declaration UUID. See
    /// [here](https://www.bluetooth.com/specifications/gatt/declarations) for valid UUIDs.
    /// Takes an optional callback that will be called with an error or the device response.
    fn read_by_type_async(&self, _characteristic: &Characteristic,
                          _uuid: UUID, _handler: Option<RequestCallback>) {
    }

    /// Sends a read-by-type request to device for the range of handles covered by the
    /// characteristic and for the specified declaration UUID. See
    /// [here](https://www.bluetooth.com/specifications/gatt/declarations) for valid UUIDs.
    /// Synchronously returns either an error or the device response.
    fn read_by_type(&self, characteristic: &Characteristic,
                    _uuid: UUID) -> Result<Vec<u8>> {
        Err(Error::NotSupported("read_by_type".into()))
    }

    /// Enables either notify or indicate (depending on support) for the specified characteristic.
    /// This is a synchronous call.
    fn subscribe(&self, characteristic: &Characteristic) -> Result<()> {
        info!("Trying to subscribe!");
        task::block_on(async {
            let fut = CoreBluetoothReplyFuture::default();
            self.message_sender.send(CoreBluetoothMessage::Subscribe(self.uuid, get_apple_uuid(characteristic.uuid), fut.get_state_clone())).await;
            match fut.await {
                CoreBluetoothReply::Ok => {},
                _ => panic!("Didn't subscribe!"),
            }
        });
        Ok(())
    }

    /// Disables either notify or indicate (depending on support) for the specified characteristic.
    /// This is a synchronous call.
    fn unsubscribe(&self, characteristic: &Characteristic) -> Result<()> {
        info!("Trying to unsubscribe!");
        Err(Error::NotSupported("unsubscribe".into()))
    }

    /// Registers a handler that will be called when value notification messages are received from
    /// the device. This method should only be used after a connection has been established. Note
    /// that the handler will be called in a common thread, so it should not block.
    fn on_notification(&self, handler: NotificationHandler) {
        let mut list = self.notification_handlers.lock().unwrap();
        list.push(handler);
    }

    fn read_async(&self, characteristic: &Characteristic, handler: Option<RequestCallback>) {
    }

    fn read(&self, characteristic: &Characteristic) -> Result<Vec<u8>> {
        Ok(vec!())
    }
}
