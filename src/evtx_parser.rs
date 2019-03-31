use crate::evtx_chunk::EvtxChunkData;
use crate::evtx_file_header::EvtxFileHeader;
use crate::evtx_record::EvtxRecord;

use failure::Error;
use log::{debug, info};

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::iter::{IntoIterator, Iterator};

use std::path::Path;
use std::vec::IntoIter;

pub const EVTX_CHUNK_SIZE: usize = 65536;
pub const EVTX_FILE_HEADER_SIZE: usize = 4096;

pub trait ReadSeek: Read + Seek {
    fn tell(&mut self) -> io::Result<u64> {
        self.seek(SeekFrom::Current(0))
    }
}

impl<T: Read + Seek> ReadSeek for T {}

pub struct EvtxParser<T: ReadSeek> {
    data: T,
    header: EvtxFileHeader,
}

impl EvtxParser<File> {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().canonicalize()?;
        let f = File::open(&path)?;

        let cursor = f;
        Self::from_read_seek(cursor)
    }
}

impl EvtxParser<Cursor<Vec<u8>>> {
    pub fn from_buffer(buffer: Vec<u8>) -> Result<Self, Error> {
        let cursor = Cursor::new(buffer);
        Self::from_read_seek(cursor)
    }
}

impl<T: ReadSeek> EvtxParser<T> {
    fn from_read_seek(mut read_seek: T) -> Result<Self, Error> {
        let evtx_header = EvtxFileHeader::from_reader(&mut read_seek)?;

        debug!("EVTX Header: {:#?}", evtx_header);
        Ok(EvtxParser {
            data: read_seek,
            header: evtx_header,
        })
    }

    pub fn allocate_chunk(data: &mut T, chunk_number: u16) -> Result<EvtxChunkData, Error> {
        let mut chunk_data = Vec::with_capacity(EVTX_CHUNK_SIZE);
        data.seek(SeekFrom::Start(
            (EVTX_FILE_HEADER_SIZE + chunk_number as usize * EVTX_CHUNK_SIZE) as u64,
        ))
        .unwrap();

        data.take(EVTX_CHUNK_SIZE as u64)
            .read_to_end(&mut chunk_data)
            .unwrap();

        EvtxChunkData::new(chunk_data)
    }

    pub fn records(mut self) -> IterRecords<T> {
        let first_chunk = Self::allocate_chunk(&mut self.data, 0).expect("Invalid chunk");

        IterRecords {
            header: self.header,
            data: self.data,
            current_chunk_number: 0,
            chunk_records: first_chunk.into_records().into_iter(),
        }
    }
}

impl<T: ReadSeek> IterRecords<T> {
    fn allocate_chunk(&mut self) {
        info!("Allocating new chunk {}", self.current_chunk_number);

        let chunk = EvtxParser::allocate_chunk(&mut self.data, self.current_chunk_number)
            .expect("Invalid chunk");

        self.chunk_records = chunk.into_records().into_iter();
    }
}

pub struct IterRecords<T: ReadSeek> {
    header: EvtxFileHeader,
    data: T,
    current_chunk_number: u16,
    chunk_records: IntoIter<Result<EvtxRecord, failure::Error>>,
}

impl<T: ReadSeek> Iterator for IterRecords<T> {
    type Item = Result<EvtxRecord, Error>;
    fn next(&mut self) -> Option<<Self as Iterator>::Item> {
        let mut next = self.chunk_records.next();

        // Need to load a new chunk.
        if next.is_none() {
            // If the next chunk is going to be more than the chunk count (which is 1 based)
            if self.current_chunk_number + 1 == self.header.chunk_count {
                return None;
            }

            self.current_chunk_number += 1;

            self.allocate_chunk();
            next = self.chunk_records.next()
        }

        next
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_variables)]

    use super::*;
    use crate::ensure_env_logger_initialized;

    fn process_90_records(buffer: &'static [u8]) {
        let parser = EvtxParser::from_buffer(buffer.to_vec()).unwrap();

        for (i, record) in parser.records().take(90).enumerate() {
            match record {
                Ok(r) => {
                    assert_eq!(r.event_record_id, i as u64 + 1);
                }
                Err(e) => println!("Error while reading record {}, {:?}", i, e),
            }
        }
    }

    // For clion profiler
    #[test]
    fn test_process_single_chunk() {
        let evtx_file = include_bytes!("../samples/security.evtx");
        process_90_records(evtx_file);
    }

    #[test]
    fn test_sample_2() {
        let evtx_file = include_bytes!("../samples/system.evtx");
        let parser = EvtxParser::from_buffer(evtx_file.to_vec()).unwrap();

        for (i, record) in parser.records().take(10).enumerate() {
            match record {
                Ok(r) => {
                    assert_eq!(
                        r.event_record_id,
                        i as u64 + 1,
                        "Parser is skipping records!"
                    );
                    println!("{}", r.data);
                }
                Err(e) => panic!("Error while reading record {}, {:?}", i, e),
            }
        }
    }

    #[test]
    fn test_parses_first_10_records() {
        ensure_env_logger_initialized();
        let evtx_file = include_bytes!("../samples/security.evtx");
        let parser = EvtxParser::from_buffer(evtx_file.to_vec()).unwrap();

        for (i, record) in parser.records().take(10).enumerate() {
            match record {
                Ok(r) => {
                    assert_eq!(
                        r.event_record_id,
                        i as u64 + 1,
                        "Parser is skipping records!"
                    );
                    println!("{}", r.data);
                }
                Err(e) => panic!("Error while reading record {}, {:?}", i, e),
            }
        }
    }

    #[test]
    fn test_parses_records_from_different_chunks() {
        ensure_env_logger_initialized();
        let evtx_file = include_bytes!("../samples/security.evtx");
        let parser = EvtxParser::from_buffer(evtx_file.to_vec()).unwrap();

        for (i, record) in parser.records().take(1000).enumerate() {
            match record {
                Ok(r) => {
                    assert_eq!(r.event_record_id, i as u64 + 1);
                    println!("{}", r.data);
                }
                Err(e) => println!("Error while reading record {}, {:?}", i, e),
            }
        }
    }

    #[test]
    fn test_parses_chunk2() {
        ensure_env_logger_initialized();
        let evtx_file = include_bytes!("../samples/security.evtx");

        let chunk = EvtxChunkData::new(
            evtx_file[EVTX_FILE_HEADER_SIZE + EVTX_CHUNK_SIZE
                ..EVTX_FILE_HEADER_SIZE + 2 * EVTX_CHUNK_SIZE]
                .to_vec(),
        )
        .unwrap();

        assert!(chunk.validate_checksum());

        for record in chunk.parse().into_iter() {
            if let Err(e) = record {
                println!("{}", e);
                panic!();
            }

            if let Ok(r) = record {
                println!("{}", r.data);
            }
        }
    }
}
