use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_std::channel::bounded;
use async_std::task;
use async_std::task::block_on;

use anyhow::Context;
use anyhow::Result;
use cid::multihash::Code;
use cid::Cid;
use fvm_shared::actor;
use fvm_shared::blockstore::{Block, Blockstore, MemoryBlockstore};
use fvm_shared::encoding::DAG_CBOR;
use ipld_car::CarHeader;

const IPLD_RAW: u64 = 0x55;

type ActorIndex = BTreeMap<fvm_shared::actor::builtin::Type, Cid>;

/// A library to bundle the Wasm bytecode of builtin actors into a CAR file.
///
/// The single root CID of the CAR file points to an CBOR-encoded IPLD
/// Map<Cid, i32> where i32 is to be interpreted as an
/// fvm_shared::actor::builtin::Type enum value.
pub struct Bundler {
    /// Staging blockstore.
    blockstore: MemoryBlockstore,
    /// Tracks the mapping of actors to Cids. Inverted when writing. Allows overriding.
    added: ActorIndex,
    /// Path of the output bundle.
    bundle_dst: PathBuf,
}

impl Bundler {
    pub fn new<P>(bundle_dst: P) -> Bundler
    where
        P: AsRef<Path>,
    {
        Bundler {
            bundle_dst: bundle_dst.as_ref().to_owned(),
            blockstore: Default::default(),
            added: Default::default(),
        }
    }

    /// Adds bytecode from a byte slice.
    pub fn add_from_bytes(
        &mut self,
        actor_type: actor::builtin::Type,
        forced_cid: Option<Cid>,
        bytecode: &[u8],
    ) -> Result<Cid> {
        let cid = match forced_cid {
            Some(cid) => {
                self.blockstore.put_keyed(&cid, bytecode).with_context(|| {
                    format!(
                        "failed to put bytecode for actor {:?} into blockstore",
                        actor_type
                    )
                })?;
                cid
            }
            None => {
                let blk = &Block {
                    codec: IPLD_RAW,
                    data: bytecode,
                };
                self.blockstore
                    .put(Code::Blake2b256, blk)
                    .with_context(|| {
                        format!(
                            "failed to put bytecode for actor {:?} into blockstore",
                            actor_type
                        )
                    })?
            }
        };
        self.added.insert(actor_type, cid);
        Ok(cid)
    }

    /// Adds bytecode from a file.
    pub fn add_from_file<P: AsRef<Path>>(
        &mut self,
        actor_type: actor::builtin::Type,
        forced_cid: Option<Cid>,
        bytecode_path: P,
    ) -> Result<Cid> {
        let bytecode = std::fs::read(bytecode_path).context("failed to open bytecode file")?;
        self.add_from_bytes(actor_type, forced_cid, bytecode.as_slice())
    }

    /// Commits the added bytecode entries and writes the CAR file to disk.
    pub fn finish(self) -> Result<()> {
        block_on(self.write_car())
    }

    async fn write_car(self) -> Result<()> {
        let mut out = async_std::fs::File::create(&self.bundle_dst).await?;

        // Invert the actor index so that it's CID => Type.
        let index: BTreeMap<_, _> = self
            .added
            .into_iter()
            .map(|(typ, cid)| (cid, typ))
            .collect();

        // Create an index data structure to use as a root.
        let index_bytes = serde_cbor::to_vec(&index)?;
        let root = self.blockstore.put(
            Code::Blake2b256,
            &Block {
                codec: DAG_CBOR,
                data: &index_bytes,
            },
        )?;

        // Create a CAR header.
        let car = CarHeader {
            roots: vec![root],
            version: 1,
        };

        let (tx, mut rx) = bounded(16);
        let write_task =
            task::spawn(async move { car.write_stream_async(&mut out, &mut rx).await.unwrap() });

        // Add the root payload.
        tx.send((root, index_bytes)).await.unwrap();

        // Add the bytecodes.
        for cid in index.iter().map(|(cid, _)| cid) {
            println!("adding cid {} to bundle CAR", cid.to_string());
            let data = self.blockstore.get(cid).unwrap().unwrap();
            tx.send((*cid, data)).await.unwrap();
        }

        drop(tx);

        write_task.await;

        Ok(())
    }
}

#[test]
fn test_bundler() {
    use async_std::fs::File;
    use cid::multihash::MultihashDigest;
    use ipld_car::{load_car, CarReader};
    use rand::Rng;

    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test_bundle.car");

    // Write 20 random payloads to the bundle.
    let mut cids = Vec::with_capacity(10);
    let mut bundler = Bundler::new(&path);

    // First 5 have real CIDs, last 5 have forced CIDs.
    for i in 1..=10 {
        let forced_cid = if i < 5 {
            None
        } else {
            Some(Cid::new_v1(
                IPLD_RAW,
                Code::Identity.digest(format!("actor-{}", i).as_bytes()),
            ))
        };
        use num_traits::FromPrimitive;
        let typ = actor::builtin::Type::from_i32(i).unwrap();
        let cid = bundler
            .add_from_bytes(typ, forced_cid, &rand::thread_rng().gen::<[u8; 32]>())
            .unwrap();

        dbg!(cid.to_string());
        cids.push(cid);
    }
    bundler.finish().unwrap();

    // Read with the CarReader directly and verify there's a single root.
    let reader = block_on(async {
        let file = File::open(&path).await.unwrap();
        CarReader::new(file).await.unwrap()
    });
    assert_eq!(reader.header.roots.len(), 1);
    dbg!(reader.header.roots[0].to_string());

    // Load the CAR into a Blockstore.
    let bs = MemoryBlockstore::default();
    let roots = block_on(async {
        let file = File::open(&path).await.unwrap();
        load_car(&bs, file).await.unwrap()
    });
    assert_eq!(roots.len(), 1);

    // Compare that the previous root matches this one.
    assert_eq!(reader.header.roots[0], roots[0]);

    // The single root represents the index.
    let index_cid = roots[0];
    let index_data = bs.get(&index_cid).unwrap().unwrap();

    // Deserialize the index.
    let index: BTreeMap<String, Cid> = serde_cbor::from_slice(index_data.as_slice()).unwrap();

    // Verify the index contains what we expect.
    for (i, (actor, cid)) in (0..20)
        .map(|i| format!("foo-{}", i).to_owned())
        .zip(cids.iter())
        .enumerate()
    {
        assert_eq!(index[&actor], *cid);
        // Verify that the last 10 CIDs are really forced CIDs.
        if i > 10 {
            let expected = Cid::new_v1(IPLD_RAW, Code::Identity.digest(actor.as_bytes()));
            assert_eq!(*cid, expected)
        }
        assert!(bs.has(cid).unwrap());
    }
}
