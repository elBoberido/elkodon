use crate::setup::*;

use elkodon::prelude::*;

use core::mem::MaybeUninit;
use std::time::{Duration, SystemTime};

pub fn run_follower_process() -> Result<(), Box<dyn std::error::Error>> {
    // follower setup
    let follower_service = zero_copy::Service::new(&ServiceName::new(FOLLOWER_SERVICE_NAME)?)
        .publish_subscribe()
        .max_publishers(1)
        .max_subscribers(1)
        .history_size(0)
        .subscriber_max_buffer_size(1)
        .enable_safe_overflow(false)
        .open_or_create::<BenchTopic<1024>>()?;

    let follower_publisher = follower_service.publisher().create()?;

    // leader setup
    let leader_service = zero_copy::Service::new(&ServiceName::new(LEADER_SERVICE_NAME)?)
        .publish_subscribe()
        .max_publishers(1)
        .max_subscribers(1)
        .history_size(0)
        .subscriber_max_buffer_size(1)
        .enable_safe_overflow(false)
        .open_or_create::<BenchTopic<1024>>()?;

    let leader_subscriber = leader_service.subscriber().create()?;

    // latency result setup
    let latency_service = zero_copy::Service::new(&ServiceName::new(LATENCY_SERVICE_NAME)?)
        .publish_subscribe()
        .max_publishers(2)
        .max_subscribers(1)
        .history_size(0)
        .subscriber_max_buffer_size(2)
        .enable_safe_overflow(false)
        .open_or_create::<LatencyTopic>()?;

    let latency_publisher = latency_service.publisher().create()?;
    let mut latency_sample = latency_publisher.loan()?;

    // ready setup
    let ready_event = zero_copy::Service::new(&ServiceName::new(READY_EVENT_NAME)?)
        .event()
        .open_or_create()?;

    let ready_notifier = ready_event.notifier().create()?;

    // signal ready to main process
    ready_notifier.notify_with_custom_event_id(FOLLOWER_READY_EVENT_ID)?;

    let mut i = 0;
    let mut finished = false;
    while !finished {
        let mut abort_counter = 100_000_000;
        let sample = loop {
            match leader_subscriber.receive() {
                Ok(None) => { /* nothing to do */ }
                Ok(Some(sample)) => {
                    break sample;
                }
                Err(e) => Err(format!("Error at receiving samples: {:?}", e))?,
            }
            abort_counter -= 1;
            if abort_counter == 0 {
                Err("The leader process is not responding")?;
            }
        };

        if !sample.info.warmup {
            let receive_timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos();
            let latency = receive_timestamp.saturating_sub(sample.info.timestamp);
            latency_sample.payload_mut().latencies[i] = latency as u64;
            finished = sample.info.last;
            i += 1;
        }

        let send_timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let sample = follower_publisher.loan_uninit()?.write_payload(BenchTopic {
            info: Info {
                timestamp: send_timestamp,
                warmup: sample.info.warmup,
                last: finished,
            },
            data: MaybeUninit::uninit(),
        });
        follower_publisher.send(sample)?;
    }
    latency_sample.payload_mut().used_size = i;
    latency_publisher.send(latency_sample)?;

    println!("Follower finished!");

    // FIXME the samples are not received when the process is gone
    std::thread::sleep(Duration::from_secs(2));

    Ok(())
}
