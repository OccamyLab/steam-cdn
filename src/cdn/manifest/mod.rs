use aes::{
    cipher::{
        block_padding::Pkcs7, generic_array::GenericArray, BlockDecrypt, BlockDecryptMut, KeyInit,
        KeyIvInit,
    },
    Aes256, Aes256Dec,
};
use buf::TryBuf;
use bytes::Bytes;
use error::ManifestError;
use std::{
    io::{Cursor, Read},
    str,
};
use steam_vent::proto::{
    content_manifest::{ContentManifestMetadata, ContentManifestPayload, ContentManifestSignature},
    protobuf::Message,
};
use zip::ZipArchive;
use base64::{prelude::BASE64_STANDARD, Engine};

use crate::crypto::base64::base64_decode;

mod buf;
pub mod error;

const PROTOBUF_PAYLOAD_MAGIC: u32 = 0x71F617D0;
const PROTOBUF_METADATA_MAGIC: u32 = 0x1F4812BE;
const PROTOBUF_SIGNATURE_MAGIC: u32 = 0x1B81B817;
const PROTOBUF_ENDOFMANIFEST_MAGIC: u32 = 0x32C415AB;

#[derive(Debug)]
pub struct ChunkData {
    sha: Vec<u8>,
    crc: u32,
    offset: u64,
    original_size: u32,
    compressed_size: u32,
}

impl ChunkData {
    pub fn sha(&self) -> Vec<u8> {
        self.sha.clone()
    }

    pub fn id(&self) -> String {
        BASE64_STANDARD.encode(&self.sha)
    }

    pub fn crc(&self) -> u32 {
        self.crc
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn original_size(&self) -> u32 {
        self.original_size
    }

    pub fn compressed_size(&self) -> u32 {
        self.compressed_size
    }
}

#[derive(Debug)]
pub struct ManifestFile {
    filename: String,
    size: u64,
    flags: u32,
    sha_filename: Vec<u8>,
    sha_content: Vec<u8>,
    chunks: Vec<ChunkData>,
    linktarget: String,
}

impl ManifestFile {
    pub fn filename(&self) -> String {
        self.filename.clone()
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn flags(&self) -> u32 {
        self.flags
    }

    pub fn sha_filename(&self) -> Vec<u8> {
        self.sha_filename.clone()
    }

    pub fn sha_content(&self) -> Vec<u8> {
        self.sha_content.clone()
    }

    pub fn chunks(&self) -> &Vec<ChunkData> {
        self.chunks.as_ref()
    }

    pub fn linktarget(&self) -> String {
        self.linktarget.clone()
    }
}

#[derive(Debug)]
pub struct DepotManifest {
    depot_id: u32,
    manifest_gid: u64,
    creatime_time: u32,
    filenames_encrypted: bool,
    original_size: u64,
    compressed_size: u64,
    files: Vec<ManifestFile>,
}

impl DepotManifest {
    pub fn depot_id(&self) -> u32 {
        self.depot_id
    }

    pub fn manifest_gid(&self) -> u64 {
        self.manifest_gid
    }

    pub fn creatime_time(&self) -> u32 {
        self.creatime_time
    }

    pub fn filenames_encrypted(&self) -> bool {
        self.filenames_encrypted
    }

    pub fn original_size(&self) -> u64 {
        self.original_size
    }

    pub fn compressed_size(&self) -> u64 {
        self.compressed_size
    }

    pub fn files(&self) -> &Vec<ManifestFile> {
        &self.files
    }

    pub fn decrypt_filenames(&mut self, key: [u8; 32]) -> Result<(), ManifestError> {
        if self.filenames_encrypted {
            for file in &mut self.files {
                let mut encrypted = base64_decode(file.filename.as_bytes())?;

                let mut iv = [0u8; 16];
                let iv_len = iv.len();
                iv.copy_from_slice(&encrypted[..iv_len]);
                Aes256Dec::new(GenericArray::from_slice(&key))
                    .decrypt_block(GenericArray::from_mut_slice(&mut iv[..]));

                let filename = str::from_utf8(
                    cbc::Decryptor::<Aes256>::new(
                        GenericArray::from_slice(&key),
                        GenericArray::from_slice(&iv),
                    )
                    .decrypt_padded_mut::<Pkcs7>(&mut encrypted[iv_len..])?,
                )?;

                file.filename = filename.to_string();
            }

            self.filenames_encrypted = false;
        }
        Ok(())
    }
}

impl TryFrom<&[u8]> for DepotManifest {
    type Error = ManifestError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        let cursor = Cursor::new(data);
        let mut buffer = Vec::new();
        ZipArchive::new(cursor)?
            .by_index(0)?
            .read_to_end(&mut buffer)?;

        let mut bytes = Bytes::from(buffer);
        if bytes.try_get_u32()? != PROTOBUF_PAYLOAD_MAGIC {
            return Err(ManifestError::MagicMismatch(
                "expecting protobuf payload".to_string(),
            ));
        }

        let payload = ContentManifestPayload::parse_from_bytes(&bytes.try_get_bytes()?)?;

        if bytes.try_get_u32()? != PROTOBUF_METADATA_MAGIC {
            return Err(ManifestError::MagicMismatch(
                "expecting protobuf metadata".to_string(),
            ));
        }

        let metadata = ContentManifestMetadata::parse_from_bytes(&bytes.try_get_bytes()?)?;

        if bytes.try_get_u32()? != PROTOBUF_SIGNATURE_MAGIC {
            return Err(ManifestError::MagicMismatch(
                "expecting protobuf signature".to_string(),
            ));
        }

        let _signature = ContentManifestSignature::parse_from_bytes(&bytes.try_get_bytes()?)?;

        if bytes.try_get_u32()? != PROTOBUF_ENDOFMANIFEST_MAGIC {
            return Err(ManifestError::MagicMismatch(
                "expecting end of manifest".to_string(),
            ));
        }

        Ok(Self {
            depot_id: metadata.depot_id(),
            manifest_gid: metadata.gid_manifest(),
            creatime_time: metadata.creation_time(),
            filenames_encrypted: metadata.filenames_encrypted(),
            original_size: metadata.cb_disk_original(),
            compressed_size: metadata.cb_disk_compressed(),
            files: payload
                .mappings
                .iter()
                .map(|map| ManifestFile {
                    filename: map.filename().to_string(),
                    size: map.size(),
                    flags: map.flags(),
                    sha_filename: map.sha_filename().to_vec(),
                    sha_content: map.sha_content().to_vec(),
                    chunks: map
                        .chunks
                        .iter()
                        .map(|chunk| ChunkData {
                            sha: chunk.sha().to_vec(),
                            crc: chunk.crc(),
                            offset: chunk.offset(),
                            original_size: chunk.cb_original(),
                            compressed_size: chunk.cb_compressed(),
                        })
                        .collect::<Vec<ChunkData>>(),
                    linktarget: map.linktarget().to_string(),
                })
                .collect::<Vec<ManifestFile>>(),
        })
    }
}
