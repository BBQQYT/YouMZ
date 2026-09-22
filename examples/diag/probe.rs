use rodio::{Decoder, OutputStream, Sink};
use std::fs::File;
use std::io::Cursor;

fn main() {
    let path = std::env::args().nth(1).expect("укажи путь к аудио");
    let bytes = std::fs::read(&path).expect("не прочитать файл");
    println!("размер: {} байт", bytes.len());

    // 1) Cursor в памяти
    match Decoder::new(Cursor::new(bytes.clone())) {
        Ok(_) => println!("Cursor: OK"),
        Err(e) => println!("Cursor: ERR {e:?}"),
    }
    // 2) File
    match Decoder::new(File::open(&path).expect("файл")) {
        Ok(_) => println!("File: OK"),
        Err(e) => println!("File: ERR {e:?}"),
    }
    // 3) попробуем воспроизвести
    match OutputStream::try_default() {
        Ok((_s, h)) => {
            let sink = Sink::try_new(&h).expect("sink");
            match Decoder::new(Cursor::new(bytes)) {
                Ok(src) => {
                    sink.append(src);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    println!("воспроизведение 2с: {}, empty={}", !sink.is_paused(), sink.empty());
                }
                Err(e) => println!("play ERR {e:?}"),
            }
        }
        Err(e) => println!("нет аудио: {e}"),
    }
}
