use esp_idf_hal::{
    delay::FreeRtos,
    gpio::PinDriver,
    i2c::{I2cConfig, I2cDriver, I2cSlaveConfig, I2cSlaveDriver},
    prelude::Peripherals,
};
use esp_idf_sys as _; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

use std::{
    cell::RefCell,
    sync::{mpsc::sync_channel, Arc, Mutex, MutexGuard, TryLockError},
};

use esp_idf_ble::{
    AdvertiseData, AttributeValue, AutoResponse, BtUuid, EspBle, GattCharacteristic,
    GattDescriptor, GattService, GattServiceEvent, ServiceUuid,
};
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    netif::{EspNetif, NetifStack},
    nvs::EspDefaultNvsPartition,
};

use esp_idf_sys::*;

use log::{info, warn};

use shared::Commands;

fn get_mac(mac: [u8; 6]) -> String {
    let mut mac_str = String::new();
    for (i, byte) in mac.iter().enumerate() {
        if i > 0 {
            mac_str.push(':');
        }
        mac_str.push_str(&format!("{:02X}", byte));
    }
    mac_str
}

fn main() -> anyhow::Result<()> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_sys::link_patches();
    let netif_stack = Arc::new(EspNetif::new(NetifStack::Sta).expect("Unable to init Netif Stack"));

    let mac = get_mac(netif_stack.get_mac().expect("Unable to get MAC address"));

    let peripherals = Peripherals::take().unwrap();

    let sda = peripherals.pins.gpio32;
    let scl = peripherals.pins.gpio33;
    let i2c = peripherals.i2c1;

    let config = I2cSlaveConfig::new()
        .rx_buffer_length(256)
        .tx_buffer_length(256);
    let mut driver = I2cSlaveDriver::new(i2c, sda, scl, 0x16, &config)?;

    // BLE
    esp_idf_svc::log::EspLogger::initialize_default();

    let commands = Arc::new(Mutex::new(RefCell::new(Vec::<Commands>::new())));
    let com = Arc::clone(&commands);
    let commands_to_send = Arc::new(Mutex::new(RefCell::new(Vec::<Commands>::new())));
    let cts = Arc::clone(&commands_to_send);

    #[allow(unused)]
    let sys_loop_stack = Arc::new(EspSystemEventLoop::take().expect("Unable to init sys_loop"));

    #[allow(unused)]
    let default_nvs = Arc::new(EspDefaultNvsPartition::take().unwrap());

    FreeRtos::delay_us(100_u32);

    let mut ble = EspBle::new("ESP32".into(), default_nvs).unwrap();

    let (s, r) = sync_channel(1);

    ble.register_gatt_service_application(1, move |gatts_if, reg| {
        if let GattServiceEvent::Register(reg) = reg {
            info!("Service registered with {:?}", reg);
            s.send(gatts_if).expect("Unable to send result");
        } else {
            warn!("What are you doing here??");
        }
    })
    .expect("Unable to register service");

    let svc_uuid = BtUuid::Uuid16(ServiceUuid::Battery as u16);

    let svc = GattService::new_primary(svc_uuid, 4, 1);

    info!("GattService to be created: {:?}", svc);

    let gatts_if = r.recv().expect("Unable to receive value");

    let (s, r) = sync_channel(1);

    ble.register_connect_handler(gatts_if, |_gatts_if, connect| {
        if let GattServiceEvent::Connect(connect) = connect {
            info!("Connect event: {:?}", connect);
        }
    });

    ble.create_service(gatts_if, svc, move |gatts_if, create| {
        if let GattServiceEvent::Create(create) = create {
            info!(
                "Service created with {{ \tgatts_if: {}\tstatus: {}\n\thandle: {}\n}}",
                gatts_if, create.status, create.service_handle
            );
            s.send(create.service_handle).expect("Unable to send value");
        }
    })
    .expect("Unable to create service");

    let svc_handle = r.recv().expect("Unable to receive value");

    ble.start_service(svc_handle, |_, start| {
        if let GattServiceEvent::StartComplete(start) = start {
            info!("Service started for handle: {}", start.service_handle);
        }
    })
    .expect("Unable to start ble service");

    let attr_value: AttributeValue<12> = AttributeValue::new_with_value(&[
        0x48, 0x65, 0x6C, 0x6C, 0x6F, 0x20, 0x57, 0x6F, 0x72, 0x6C, 0x64,
    ]);
    let charac = GattCharacteristic::new(
        BtUuid::Uuid16(0xff01),
        (ESP_GATT_PERM_READ | ESP_GATT_PERM_WRITE) as _,
        (ESP_GATT_CHAR_PROP_BIT_READ | ESP_GATT_CHAR_PROP_BIT_WRITE) as _,
        attr_value,
        AutoResponse::ByApp,
    );

    let (s, r) = sync_channel(1);

    ble.add_characteristic(svc_handle, charac, move |_, add_char| {
        if let GattServiceEvent::AddCharacteristicComplete(add_char) = add_char {
            info!("Attr added with handle: {}", add_char.attr_handle);
            s.send(add_char.attr_handle).expect("Unable to send value");
        }
    })
    .expect("Unable to add characteristic");

    let char_attr_handle = r.recv().expect("Unable to recv attr_handle");

    let data = ble
        .read_attribute_value(char_attr_handle)
        .expect("Unable to read characteristic value");
    info!("Characteristic values: {:?}", data);

    let cdesc = GattDescriptor::new(
        BtUuid::Uuid16(ESP_GATT_UUID_CHAR_CLIENT_CONFIG as u16),
        ESP_GATT_PERM_READ as _,
    );
    ble.add_descriptor(svc_handle, cdesc, |_, add_desc| {
        if let GattServiceEvent::AddDescriptorComplete(add_desc) = add_desc {
            info!("Descriptor added with handle: {}", add_desc.attr_handle);
        }
    })
    .expect("Unable to add characteristic");

    ble.register_read_handler(char_attr_handle, move |gatts_if, read| {
        if let GattServiceEvent::Read(read) = read {
            let next_command = commands
                .try_lock()
                .ok()
                .and_then(|commands| commands.borrow_mut().pop())
                .unwrap_or_default();

            esp_idf_ble::send(
                gatts_if,
                char_attr_handle,
                read.conn_id,
                read.trans_id,
                esp_gatt_status_t_ESP_GATT_OK,
                next_command.get_stream().as_slice(),
            )
            .expect("Unable to send read response");
        }
    });

    ble.register_write_handler(char_attr_handle, move |gatts_if, write| {
        if let GattServiceEvent::Write(write) = write {
            if write.is_prep {
                warn!("Unsupported write");
            } else {
                let value = unsafe { std::slice::from_raw_parts(write.value, write.len as usize) };
                let back = Commands::parse(value)
                    .ok()
                    .and_then(|command| {
                        commands_to_send.try_lock().ok().and_then(|commands| {
                            commands.borrow_mut().insert(0, command);
                            Some(Commands::OK)
                        })
                    })
                    .unwrap_or_default();

                if write.need_rsp {
                    info!("need rsp");
                    esp_idf_ble::send(
                        gatts_if,
                        char_attr_handle,
                        write.conn_id,
                        write.trans_id,
                        esp_gatt_status_t_ESP_GATT_OK,
                        back.get_stream().as_slice(),
                    )
                    .expect("Unable to send response");
                }
            }
        }
    });

    let adv_data = AdvertiseData {
        include_name: true,
        include_txpower: false,
        min_interval: 6,
        max_interval: 16,
        service_uuid: Some(BtUuid::Uuid128([
            0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0xFF, 0x00,
            0x00, 0x00,
        ])),
        flag: (ESP_BLE_ADV_FLAG_GEN_DISC | ESP_BLE_ADV_FLAG_BREDR_NOT_SPT) as _,
        ..Default::default()
    };
    ble.configure_advertising_data(adv_data, |_| {
        info!("advertising configured");
    })
    .expect("Failed to configure advertising data");

    let scan_rsp_data = AdvertiseData {
        include_name: false,
        include_txpower: true,
        set_scan_rsp: true,
        service_uuid: Some(BtUuid::Uuid128([
            0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0xFF, 0x00,
            0x00, 0x00,
        ])),
        ..Default::default()
    };

    ble.configure_advertising_data(scan_rsp_data, |_| {
        info!("Advertising configured");
    })
    .expect("Failed to configure advertising data");

    ble.start_advertise(|_| {
        info!("advertising started");
    })?;

    loop {
        cts.try_lock()
            .ok()
            .and_then(|commands| commands.borrow_mut().pop())
            .and_then(|command| driver.write(command.get_stream().as_slice(), 200).ok());
        let mut buffer = [0u8; 256];
        if driver.read(&mut buffer, 50).is_ok() {
            Commands::parse(&buffer)
                .ok()
                .and_then(|command| {
                    println!("Command: {:?}", command);
                    match command {
                        Commands::GetMac => {
                            driver
                                .write(
                                    Commands::Mac(String::from(&mac)).get_stream().as_slice(),
                                    100,
                                )
                                .ok();
                        }
                        _ => {}
                    }
                    Some(())
                })
                .or_else(|| {
                    println!("Unable to parse command");
                    Some(())
                });
        }

        FreeRtos::delay_ms(50);
    }
}
