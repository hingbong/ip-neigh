use std::{fmt::Error, path::Path};

use hickory_proto::{
    rr::Record,
    serialize::binary::{BinDecodable, BinDecoder, BinEncodable, BinEncoder},
};
use rusqlite::ToSql;

pub(crate) struct SqlitePersistence {
    conn: rusqlite::Connection,
}

impl SqlitePersistence {
    pub fn new(path: &Path) -> SqlitePersistence {
        Self {
            conn: rusqlite::Connection::open(path).unwrap(),
        }
    }

    pub fn insert_record(&self, soa_serial: u32, record: &Record) -> Result<(), Error> {
        let mut serial_record: Vec<u8> = Vec::with_capacity(512);
        {
            let mut encoder = BinEncoder::new(&mut serial_record);
            record.emit(&mut encoder)?;
        }

        let timestamp = time::OffsetDateTime::now_utc();
        let client_id: i64 = 0; // TODO: we need better id information about the client, like pub_key
        let soa_serial: i64 = i64::from(soa_serial);

        let count = self
            .conn
            .execute(
                "INSERT
                                          \
                                            INTO records (client_id, soa_serial, timestamp, \
                                            record)
                                          \
                                            VALUES ($1, $2, $3, $4)",
                [
                    &client_id as &dyn ToSql,
                    &soa_serial,
                    &timestamp,
                    &serial_record,
                ],
            )
            .unwrap();
        //
        if count != 1 {
            return Err(Error::default());
        };

        Ok(())
    }

    pub fn select_record(&self, row_id: i64) -> Result<Option<(i64, Record)>, PersistenceError> {
        let conn = self.conn;
        let mut stmt = conn.prepare(
            "SELECT _rowid_, record
                                            \
                                               FROM records
                                            \
                                               WHERE _rowid_ >= $1
                                            \
                                               LIMIT 1",
        )?;

        let record_opt: Option<Result<(i64, Record), rusqlite::Error>> = stmt
            .query_and_then([&row_id], |row| -> Result<(i64, Record), rusqlite::Error> {
                let row_id: i64 = row.get(0)?;
                let record_bytes: Vec<u8> = row.get(1)?;
                let mut decoder = BinDecoder::new(&record_bytes);

                // todo add location to this...
                match Record::read(&mut decoder) {
                    Ok(record) => Ok((row_id, record)),
                    Err(decode_error) => Err(rusqlite::Error::InvalidParameterName(format!(
                        "could not decode: {decode_error}"
                    ))),
                }
            })?
            .next();

        //
        match record_opt {
            Some(Ok((row_id, record))) => Ok(Some((row_id, record))),
            Some(Err(err)) => Err(err.into()),
            None => Ok(None),
        }
    }
}
