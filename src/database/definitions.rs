use std::collections::VecDeque;

use crossbeam::channel::{unbounded, Sender, Receiver};
use crate::PartialMessage;

pub struct Database {
    messages: VecDeque<PartialMessage>,
    sender: Sender<Vec<PartialMessage>>,
    receiver: Receiver<DatabaseMessage>,
}

impl Database {
    pub fn new() -> (Sender<DatabaseMessage>, Receiver<Vec<PartialMessage>>) {
        let (db_message_sender, db_message_receiver) = unbounded();
        let (msg_sender, msg_receiver) = unbounded();
        let mut database = Self {
            messages: VecDeque::new(),
            sender: msg_sender,
            receiver: db_message_receiver
        };
        std::thread::spawn(move || {
           database.update();
        });
        (db_message_sender, msg_receiver)
    }

    fn update(&mut self) {
        while let Ok(message) = self.receiver.recv() {
            match message {
                DatabaseMessage::InsertMessage(message) => {
                    let context_size = std::env::var("CONTEXT_SIZE").unwrap().parse::<usize>().unwrap();
                    if self.messages.len() >= context_size {
                        self.messages.pop_front();
                    }
                    self.messages.push_back(message);
                    println!("{:?}", self.messages);
                },
                DatabaseMessage::GetLatest(n_latest) => {
                    let start = std::cmp::max(n_latest as usize, self.messages.len()) - (n_latest as usize);
                    let slice = self.messages.as_slices().0[start..].to_vec();
                    let _ = self.sender.send(slice);
                },
                DatabaseMessage::ValidateEntries(n_latest) => {
                    let start = std::cmp::max(n_latest as usize, self.messages.len()) - (n_latest as usize);
                    let slice = &mut self.messages.as_mut_slices().0[start..];
                    slice.iter_mut().for_each(|x| x.status = "validated".into());
                }
            }
        }
    }
}

pub enum DatabaseMessage {
    GetLatest(u8),
    InsertMessage(PartialMessage),
    ValidateEntries(u64)
}
