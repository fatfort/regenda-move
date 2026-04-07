use super::display::{INPUT_TOUCH_PRESS, INPUT_TOUCH_RELEASE, INPUT_TOUCH_UPDATE, MESSAGE_USERINPUT};
use super::types::{Finger, InputEvent, MultitouchEvent};
use cgmath::Point2;
use std::os::unix::io::RawFd;
use std::sync::mpsc::Sender;
use std::thread;

const MSG_BUF_SIZE: usize = 64;

/// Start a thread that receives QTFB input events from the socket
/// and translates them to our InputEvent type.
pub fn start_input_threads(tx: Sender<InputEvent>, qtfb_fd: RawFd) {
    thread::spawn(move || {
        log::info!("QTFB input thread started on fd {}", qtfb_fd);

        let mut buf = [0u8; MSG_BUF_SIZE];
        let mut last_tracking_id: i32 = 0;

        loop {
            let n = unsafe {
                libc::recv(
                    qtfb_fd,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };

            if n <= 0 {
                if n == 0 {
                    log::info!("QTFB socket closed");
                } else {
                    log::error!(
                        "QTFB recv error: {}",
                        std::io::Error::last_os_error()
                    );
                }
                break;
            }

            let msg_type = buf[0];
            if msg_type != MESSAGE_USERINPUT {
                continue;
            }

            let offset = if n >= 28 { 8usize } else { 4usize };

            let input_type = i32::from_ne_bytes([
                buf[offset],
                buf[offset + 1],
                buf[offset + 2],
                buf[offset + 3],
            ]);
            let x = i32::from_ne_bytes([
                buf[offset + 8],
                buf[offset + 9],
                buf[offset + 10],
                buf[offset + 11],
            ]);
            let y = i32::from_ne_bytes([
                buf[offset + 12],
                buf[offset + 13],
                buf[offset + 14],
                buf[offset + 15],
            ]);

            let finger = Finger {
                pos: Point2 {
                    x: x.max(0) as u16,
                    y: y.max(0) as u16,
                },
                tracking_id: last_tracking_id,
            };

            let event = match input_type {
                t if t == INPUT_TOUCH_PRESS => {
                    last_tracking_id += 1;
                    let finger = Finger {
                        tracking_id: last_tracking_id,
                        ..finger
                    };
                    Some(InputEvent::MultitouchEvent {
                        event: MultitouchEvent::Press { finger },
                    })
                }
                t if t == INPUT_TOUCH_RELEASE => Some(InputEvent::MultitouchEvent {
                    event: MultitouchEvent::Release { finger },
                }),
                t if t == INPUT_TOUCH_UPDATE => Some(InputEvent::MultitouchEvent {
                    event: MultitouchEvent::Move { finger },
                }),
                _ => None,
            };

            if let Some(ev) = event {
                let _ = tx.send(ev);
            }
        }

        log::info!("QTFB input thread exiting");
    });
}
