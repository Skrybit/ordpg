use super::*;
use axum_server::Handle;
use crate::subcommand::server;
use crate::index::fetcher;

use mysql_async::TxOpts;
use mysql_async::Pool;
use mysql_async::prelude::Queryable;
use mysql_async::params;
use tokio::sync::Semaphore;
use tokio::sync::Mutex;
use serde::Serialize;
use sha256::digest;

use s3::operation::get_object::GetObjectOutput;
use aws_sdk_s3 as s3;	
use s3::primitives::ByteStream;	
use s3::error::ProvideErrorMetadata;

use axum::{
  routing::get,
  Json, 
  Router,
  extract::{Path, State},
  body::{Body, BoxBody},
  middleware::map_response
};

use tower_http::trace::TraceLayer;
use tower_http::trace::DefaultMakeSpan;
use tracing::Span;
use http::{Request, Response};
use tracing::Level as TraceLevel;

use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::thread::JoinHandle;
use rand::Rng;
use rand::SeedableRng;



#[derive(Debug, Parser, Clone)]
pub(crate) struct Vermilion {
  #[clap(
    long,
    default_value = "0.0.0.0",
    help = "Listen on <ADDRESS> for incoming requests."
  )]
  address: String,
  #[clap(
    long,
    help = "Request ACME TLS certificate for <ACME_DOMAIN>. This ord instance must be reachable at <ACME_DOMAIN>:443 to respond to Let's Encrypt ACME challenges."
  )]
  acme_domain: Vec<String>,
  #[clap(
    long,
    help = "Listen on <HTTP_PORT> for incoming HTTP requests. [default: 80]."
  )]
  http_port: Option<u16>,
  #[clap(
    long,
    group = "port",
    help = "Listen on <HTTPS_PORT> for incoming HTTPS requests. [default: 443]."
  )]
  https_port: Option<u16>,
  #[clap(long, help = "Store ACME TLS certificates in <ACME_CACHE>.")]
  acme_cache: Option<PathBuf>,
  #[clap(long, help = "Provide ACME contact <ACME_CONTACT>.")]
  acme_contact: Vec<String>,
  #[clap(long, help = "Serve HTTP traffic on <HTTP_PORT>.")]
  http: bool,
  #[clap(long, help = "Serve HTTPS traffic on <HTTPS_PORT>.")]
  https: bool,
  #[clap(long, help = "Redirect HTTP traffic to HTTPS.")]
  redirect_http_to_https: bool,
  #[clap(
    long,
    help = "Listen on <HTTP_PORT> for incoming REST requests. [default: 81]."
  )]
  api_http_port: Option<u16>,
  #[clap(
    long,
    help = "Number of threads to use when uploading content and metadata. [default: 1]."
  )]
  n_threads: Option<u16>,
  #[clap(long, help = "Only run api server, do not run indexer. [default: false].")]
  pub run_api_server_only: bool
}

#[derive(Clone, Serialize)]
pub struct Metadata {
  id: String,
  content_length: Option<i64>,
  content_type: Option<String>,
  genesis_fee: i64,
  genesis_height: i64,
  genesis_transaction: String,
  location: String,
  number: i64,
  sequence_number: Option<u64>,
  offset: i64,
  output_transaction: String,
  sat: Option<i64>,
  timestamp: i64,
  sha256: Option<String>,
  text: Option<String>,
  is_json: bool,
  is_maybe_json: bool,
  is_bitmap_style: Option<bool>,
  is_recursive: Option<bool>
}

#[derive(Clone, Serialize)]
pub struct SatMetadata {
  sat: u64,
  decimal: String,
  degree: String,
  name: String,
  block: u64,
  cycle: u64,
  epoch: u64,
  period: u64,
  offset: u64,
  rarity: String,
  percentile: String,
  satpoint: String,
  timestamp: i64
}

#[derive(Clone, Serialize)]
pub struct Transfer {
  id: String,
  block_number: i64,
  block_timestamp: i64,
  satpoint: String,
  transaction: String,
  address: String,
  is_genesis: bool
}

#[derive(Clone, Serialize)]
pub struct TransferWithMetadata {
  id: String,
  block_number: i64,
  block_timestamp: i64,
  satpoint: String,
  transaction: String,
  address: String,
  is_genesis: bool,
  content_length: Option<i64>,
  content_type: Option<String>,
  genesis_fee: i64,
  genesis_height: i64,
  genesis_transaction: String,
  location: String,
  number: i64,
  sequence_number: Option<u64>,
  offset: i64,
  output_transaction: String,
  sat: Option<i64>,
  timestamp: i64,
  sha256: Option<String>,
  text: Option<String>,
  is_json: bool,
  is_maybe_json: bool,
  is_bitmap_style: Option<bool>,
  is_recursive: Option<bool>
}

#[derive(Clone, Serialize)]
pub struct Content {
  content: Vec<u8>,
  content_type: Option<String>
}

#[derive(Clone, Serialize)]
pub struct InscriptionNumberEdition {
  id: String,
  number: i64,
  edition: u64
}

#[derive(Clone, Serialize)]
pub struct InscriptionMetadataForBlock {
  id: String,
  content_length: Option<i64>,
  content_type: Option<String>,
  genesis_fee: i64,
  genesis_height: i64,
  number: i64,
  timestamp: i64
}

#[derive(Clone, Serialize)]
pub struct RandomInscriptionNumber {
  number: i64
}

pub struct SequenceNumberStatus {
  sequence_number: u64,
  status: String
}

#[derive(Clone,PartialEq, PartialOrd, Ord, Eq)]
pub struct IndexerTimings {
  inscription_start: u64,
  inscription_end: u64,
  acquire_permit_start: Instant,
  acquire_permit_end: Instant,
  get_numbers_start: Instant,
  get_numbers_end: Instant,
  get_id_start: Instant,
  get_id_end: Instant,
  get_inscription_start: Instant,
  get_inscription_end: Instant,
  upload_content_start: Instant,
  upload_content_end: Instant,
  get_metadata_start: Instant,
  get_metadata_end: Instant,
  retrieval: Duration,
  insertion: Duration,
  locking: Duration
}

#[derive(Clone)]
pub struct ApiServerConfig {
  pool: mysql_async::Pool,
  s3client: s3::Client,
  bucket_name: String
}

impl Vermilion {
  pub(crate) fn run(self, options: Options) -> SubcommandResult {
    //1. Run Vermilion Server
    println!("Vermilion Server Starting");
    let vermilion_server_clone = self.clone();
    let vermilion_server_thread = vermilion_server_clone.run_vermilion_server(options.clone());

    if self.run_api_server_only {//If only running api server, block here, early return on ctrl-c
      let rt = Runtime::new().unwrap();
      rt.block_on(async {
        loop {            
          if SHUTTING_DOWN.load(atomic::Ordering::Relaxed) {
            break;
          }
          tokio::time::sleep(Duration::from_secs(10)).await;
        }          
      });
      return Ok(Box::new(Empty {}) as Box<dyn Output>);
    }

    //2. Run Ordinals Server
    println!("Ordinals Server Starting");
    let index = Arc::new(Index::open(&options)?);
    let handle = axum_server::Handle::new();
    LISTENERS.lock().unwrap().push(handle.clone());
    let ordinals_server_clone = self.clone();
    let ordinals_server_thread = ordinals_server_clone.run_ordinals_server(options.clone(), index.clone(), handle);

    //3. Run Address Indexer
    println!("Address Indexer Starting");
    let address_indexer_clone = self.clone();
    let address_indexer_thread = address_indexer_clone.run_address_indexer(options.clone(), index.clone());

    //4. Run Inscription Indexer
    println!("Inscription Indexer Starting");
    let inscription_indexer_clone = self.clone();
    inscription_indexer_clone.run_inscription_indexer(options.clone(), index.clone()); //this blocks
    println!("Inscription Indexer Stopped");

    //Wait for other threads to finish before exiting
    // vermilion_server_thread.join().unwrap();
    ordinals_server_thread.join().unwrap();
    address_indexer_thread.join().unwrap();
    Ok(Box::new(Empty {}) as Box<dyn Output>)
  }

  pub(crate) fn run_inscription_indexer(self, options: Options, index: Arc<Index>) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
      let config = options.load_config().unwrap();
      let url = config.db_connection_string.unwrap();
      let pool = Pool::new(url.as_str());
      let start_number_override = config.start_number_override;
      let s3_config = aws_config::from_env().load().await;
      let s3client = s3::Client::new(&s3_config);
      let s3_bucket_name = config.s3_bucket_name.unwrap();
      let s3_upload_start_number = config.s3_upload_start_number.unwrap_or(0);
      let s3_head_check = config.s3_head_check.unwrap_or(false);
      let n_threads = self.n_threads.unwrap_or(1).into();
      let sem = Arc::new(Semaphore::new(n_threads));
      let status_vector: Arc<Mutex<Vec<SequenceNumberStatus>>> = Arc::new(Mutex::new(Vec::new()));
      let timing_vector: Arc<Mutex<Vec<IndexerTimings>>> = Arc::new(Mutex::new(Vec::new()));
      Self::create_metadata_table(&pool).await.unwrap();
      Self::create_sat_table(&pool).await.unwrap();
      Self::create_procedure_log(pool.clone()).await.unwrap();
      Self::create_edition_procedure(pool.clone()).await.unwrap();
      Self::create_weights_procedure(pool.clone()).await.unwrap();
      let start_number = match start_number_override {
        Some(start_number_override) => start_number_override,
        None => Self::get_last_number(&pool).await.unwrap() + 1
      };
      println!("Metadata in db assumed populated up to: {:?}, will only upload metadata for {:?} onwards.", start_number.checked_sub(1), start_number);
      println!("Inscriptions in s3 assumed populated up to: {:?}, will only upload content for {:?} onwards.", std::cmp::max(s3_upload_start_number, start_number).checked_sub(1), std::cmp::max(s3_upload_start_number, start_number));
      let initial = SequenceNumberStatus {
        sequence_number: start_number,
        status: "UNKNOWN".to_string()
      };
      status_vector.lock().await.push(initial);

      // every iteration fetches 1k inscriptions
      let time = Instant::now();
      print!("Starting @ {:?}", time);
      loop {
        let t0 = Instant::now();
        //break if ctrl-c is received
        if SHUTTING_DOWN.load(atomic::Ordering::Relaxed) {
          break;
        }
        let permit = Arc::clone(&sem).acquire_owned().await;
        let cloned_index = index.clone();
        let cloned_pool = pool.clone();        
        let cloned_s3client = s3client.clone();
        let cloned_bucket_name = s3_bucket_name.clone();
        let cloned_status_vector = status_vector.clone();
        let cloned_timing_vector = timing_vector.clone();
        let fetcher =  match fetcher::Fetcher::new(&options) {
          Ok(fetcher) => fetcher,
          Err(e) => {
            println!("Error creating fetcher: {:?}, waiting a minute", e);
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
          }
        };//Need a new fetcher for each thread
        tokio::task::spawn(async move {
          let t1 = Instant::now();
          let _permit = permit;
          let needed_numbers = Self::get_needed_sequence_numbers(cloned_status_vector.clone()).await;
          let mut should_sleep = false;
          let first_number = needed_numbers[0];
          let mut last_number = needed_numbers[needed_numbers.len()-1];
          log::info!("Trying Numbers: {:?}-{:?}", first_number, last_number);          

          //1. Get ids
          let t2 = Instant::now();
          let mut inscription_ids: Vec<InscriptionId> = Vec::new();
          for j in needed_numbers.clone() {
            let inscription_id = cloned_index.get_inscription_id_by_sequence_number(j).unwrap();
            match inscription_id {
              Some(inscription_id) => {
                inscription_ids.push(inscription_id);
              },
              None => {
                log::info!("No inscription found for inscription number: {}. Marking as not found. Breaking from loop, sleeping a minute", j);
                last_number = j;
                let status_vector = cloned_status_vector.clone();
                let mut locked_status_vector = status_vector.lock().await;
                for l in needed_numbers.clone() {                  
                  let status = locked_status_vector.iter_mut().find(|x| x.sequence_number == l).unwrap();
                  if l >= j {
                    status.status = "NOT_FOUND_LOCKED".to_string();
                  }                
                }
                should_sleep = true;
                break;
              }
            }
          }
          
          //2. Get inscriptions
          let t3 = Instant::now();
          let cloned_ids = inscription_ids.clone();
          let txs = fetcher.get_transactions(cloned_ids.into_iter().map(|x| x.txid).collect()).await;
          let err_txs = match txs {
              Ok(txs) => Some(txs),
              Err(error) => {
                println!("Error getting transactions {}-{}: {:?}", first_number, last_number, error);
                let status_vector = cloned_status_vector.clone();
                { //Enclosing braces to drop the mutex so sleep doesn't block
                  let mut locked_status_vector = status_vector.lock().await;
                  for j in needed_numbers.clone() {                  
                    let status = locked_status_vector.iter_mut().find(|x| x.sequence_number == j).unwrap();
                    status.status = "ERROR".to_string();
                  }
                }
                println!("error string: {}", error.to_string());
                if  error.to_string().contains("Failed to fetch raw transaction") || 
                    error.to_string().contains("connection closed") || 
                    error.to_string().contains("error trying to connect") || 
                    error.to_string().contains("end of file") {
                  println!("Pausing for 60s & Breaking from loop");
                  //std::mem::drop(locked_status_vector); //Drop the mutex so sleep doesn't block
                  tokio::time::sleep(Duration::from_secs(60)).await;
                }
                return;
              }
          };
          let clean_txs = err_txs.unwrap();
          let cloned_ids = inscription_ids.clone();
          let id_txs: Vec<_> = cloned_ids.into_iter().zip(clean_txs.into_iter()).collect();
          let mut inscriptions: Vec<Inscription> = Vec::new();
          for (inscription_id, tx) in id_txs {
            let inscription = ParsedEnvelope::from_transaction(&tx)
              .into_iter()
              .nth(inscription_id.index as usize)
              .map(|envelope| envelope.payload)
              .unwrap();
            inscriptions.push(inscription);
          }

          //3. Upload ordinal content to s3
          let t4 = Instant::now();
          let cloned_ids = inscription_ids.clone();
          let cloned_inscriptions = inscriptions.clone();
          let number_id_inscriptions: Vec<_> = needed_numbers.clone().into_iter()
            .zip(cloned_ids.into_iter())
            .zip(cloned_inscriptions.into_iter())
            .map(|((x, y), z)| (x, y, z))
            .collect();          
          for (number, inscription_id, inscription) in number_id_inscriptions {
            if number < s3_upload_start_number {
                continue;
            }
            Self::upload_ordinal_content(&cloned_s3client, &cloned_bucket_name, inscription_id, inscription, s3_head_check).await;	//TODO: Handle errors
          }
          
          //4. Get ordinal metadata
          let t5 = Instant::now();
          let status_vector = cloned_status_vector.clone();
          let cloned_ids = inscription_ids.clone();
          let cloned_inscriptions = inscriptions.clone();
          
          let id_inscriptions: Vec<_> = cloned_ids.into_iter().zip(cloned_inscriptions.into_iter()).collect();
          let mut retrieval = Duration::from_millis(0);
          let mut metadata_vec: Vec<Metadata> = Vec::new();
          let mut sat_metadata_vec: Vec<SatMetadata> = Vec::new();
          for (inscription_id, inscription) in id_inscriptions {
            let t0 = Instant::now();
            let (metadata, sat_metadata) = Self::extract_ordinal_metadata(cloned_index.clone(), inscription_id, inscription.clone()).unwrap();
            metadata_vec.push(metadata);            
            match sat_metadata {
              Some(sat_metadata) => {
                sat_metadata_vec.push(sat_metadata);
              },
              None => {}                
            }
            let t1 = Instant::now();            
            retrieval += t1.duration_since(t0);
          }
          //4.1 Insert metadata
          let t51 = Instant::now();
          let insert_result = Self::bulk_insert_metadata(&cloned_pool.clone(), metadata_vec).await;
          let sat_insert_result = Self::bulk_insert_sat_metadata(&cloned_pool.clone(), sat_metadata_vec).await;
          //4.2 Update status
          let t52 = Instant::now();
          if insert_result.is_err() || sat_insert_result.is_err() {
            println!("Error bulk inserting metadata for inscription numbers: {}-{}. Marking as error", first_number, last_number);
            let mut locked_status_vector = status_vector.lock().await;
            for j in needed_numbers.clone() {              
              let status = locked_status_vector.iter_mut().find(|x| x.sequence_number == j).unwrap();
              status.status = "ERROR".to_string();
            }
          } else {
            let mut locked_status_vector = status_vector.lock().await;
            for j in needed_numbers.clone() {              
              let status = locked_status_vector.iter_mut().find(|x| x.sequence_number == j).unwrap();
              if status.status != "NOT_FOUND_LOCKED".to_string() {
                status.status = "SUCCESS".to_string();
              } else if status.status == "NOT_FOUND_LOCKED".to_string() {
                status.status = "NOT_FOUND".to_string();
              }
            }
          }
          
          //5. Log timings
          let t6 = Instant::now();
          if first_number != last_number {
            println!("Finished numbers {} - {} @ {:?}", first_number, last_number, t5);
          }
          let timing = IndexerTimings {
            inscription_start: first_number,
            inscription_end: last_number,
            acquire_permit_start: t0,
            acquire_permit_end: t1,
            get_numbers_start: t1,
            get_numbers_end: t2,
            get_id_start: t2,
            get_id_end: t3,
            get_inscription_start: t3,
            get_inscription_end: t4,
            upload_content_start: t4,
            upload_content_end: t5,
            get_metadata_start: t5,
            get_metadata_end: t6,
            retrieval: retrieval,
            insertion: t52.duration_since(t51),
            locking: t6.duration_since(t52)
          };
          cloned_timing_vector.lock().await.push(timing);
          Self::print_index_timings(cloned_timing_vector, n_threads as u32).await;

          //6. Sleep thread if up to date.
          if should_sleep {
            tokio::time::sleep(Duration::from_secs(60)).await;
          }
        });        
        
      }
    })
  }

  pub(crate) fn run_address_indexer(self, options: Options, index: Arc<Index>) -> JoinHandle<()> {
    let address_indexer_thread = thread::spawn(move ||{
      let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
      rt.block_on(async move {
        let config = options.load_config().unwrap();
        let url = config.db_connection_string.unwrap();
        let pool = Pool::new(url.as_str());
        Self::create_transfers_table(&pool).await.unwrap();
        Self::create_address_table(&pool).await.unwrap();

        let fetcher = fetcher::Fetcher::new(&options).unwrap();
        let first_height = options.first_inscription_height();
        let db_height = Self::get_start_block(&pool).await.unwrap();
        let mut height = std::cmp::max(first_height, db_height);
        println!("start height: {:?}", height);
        loop {
          // break if ctrl-c is received
          if SHUTTING_DOWN.load(atomic::Ordering::Relaxed) {
            break;
          }

          // make sure block is indexed before requesting transfers
          let indexed_height = index.get_blocks_indexed().unwrap();
          if height > indexed_height {
            log::warn!("Requesting block transfers for block: {:?}, only indexed up to: {:?}. Waiting a minute", height, indexed_height);
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
          }

          let transfers = match index.get_transfers_by_block_height(height) {
            Ok(transfers) => transfers,
            Err(err) => {
              println!("Error getting transfers for block height: {:?} - {:?}, waiting a minute", height, err);
              tokio::time::sleep(Duration::from_secs(60)).await;
              continue;
            }
          };

          if transfers.len() == 0 {
            height += 1;
            continue;
          }

          let txs = match fetcher.get_transactions(transfers.clone().into_iter().map(|(_id, satpoint)| satpoint.outpoint.txid).collect()).await {
            Ok(txs) => {
              txs.into_iter().map(|tx| Some(tx)).collect::<Vec<_>>()
            }
            Err(e) => {
              println!("Error getting transfer transactions for block height: {:?} - {:?}", height, e);
              if e.to_string().contains("No such mempool or blockchain transaction") {
                println!("Attempting 1 at a time");
                let mut txs = Vec::new();
                for (id, satpoint) in transfers.clone() {
                  let tx = match fetcher.get_transactions(vec![satpoint.outpoint.txid]).await {
                    Ok(mut tx) => Some(tx.pop().unwrap()),
                    Err(e) => {
                      println!("Error getting transfer transaction: {:?} for {:?} - {:?}, skipping", satpoint.outpoint.txid, id, e);
                      None
                    }
                  };
                  txs.push(tx)
                }
                txs
              } else {
                println!("Waiting a minute");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
              }              
            }
          };

          let iterator=transfers
            .into_iter()
            .zip(txs.into_iter())
            .map(|((a,b),c)| (a,b,c));
          
          let mut id_point_address = Vec::new();
          for (id, satpoint, tx) in iterator {
            let address = if satpoint.outpoint == unbound_outpoint() {
              "unbound".to_string()
            } else {
              let output = tx
                .unwrap()
                .output
                .into_iter()
                .nth(satpoint.outpoint.vout.try_into().unwrap())
                .unwrap();
              options
                .chain()
                .address_from_script(&output.script_pubkey)
                .map(|address| address.to_string())
                .unwrap_or_else(|e| e.to_string())
            };
            id_point_address.push((id, satpoint, address));
          }

          let block_time = index.block_time(Height(height)).unwrap();
          let mut transfer_vec = Vec::new();
          for (id, point, address) in id_point_address {
            let transfer = Transfer {
              id: id.to_string(),
              block_number: height.try_into().unwrap(),
              block_timestamp: block_time.timestamp().timestamp_millis(),
              satpoint: point.to_string(),
              transaction: point.outpoint.txid.to_string(),
              address: address,
              is_genesis: point.outpoint.txid == id.txid && point.outpoint.vout == id.index
            };
            transfer_vec.push(transfer);
          }
          Self::bulk_insert_transfers(&pool, transfer_vec.clone()).await.unwrap();
          Self::bulk_insert_addresses(&pool, transfer_vec).await.unwrap();
          height += 1;
        }
        println!("Address indexer stopped");
      })
    });
    return address_indexer_thread;
  }

  pub(crate) fn run_vermilion_server(self, options: Options) -> JoinHandle<()> {
    let verm_server_thread = thread::spawn(move ||{
      let rt = Runtime::new().unwrap();
      rt.block_on(async move {
        let config = options.load_config().unwrap();
        let url = config.db_connection_string.unwrap();
        let pool = mysql_async::Pool::new(url.as_str());
        let bucket_name = config.s3_bucket_name.unwrap();
        let s3_config = aws_config::from_env().load().await;
        let s3client = s3::Client::new(&s3_config);
        
        let server_config = ApiServerConfig {
          pool: pool,
          s3client: s3client,
          bucket_name: bucket_name
        };

        let app = Router::new()
          .route("/", get(Self::root))
          .route("/home", get(Self::home))
          .route("/inscription/:inscription_id", get(Self::inscription))
          .route("/inscription_number/:number", get(Self::inscription_number))
          .route("/inscription_sha256/:sha256", get(Self::inscription_sha256))
          .route("/inscription_metadata/:inscription_id", get(Self::inscription_metadata))
          .route("/inscription_metadata_number/:number", get(Self::inscription_metadata_number))
          .route("/inscription_editions/:inscription_id", get(Self::inscription_editions))
          .route("/inscription_editions_number/:number", get(Self::inscription_editions_number))
          .route("/inscription_editions_sha256/:sha256", get(Self::inscription_editions_sha256))
          .route("/inscriptions_in_block/:block", get(Self::inscriptions_in_block))
          .route("/random_inscription", get(Self::random_inscription))
          .route("/inscription_last_transfer/:inscription_id", get(Self::inscription_last_transfer))
          .route("/inscription_last_transfer_number/:number", get(Self::inscription_last_transfer_number))
          .route("/inscription_transfers/:inscription_id", get(Self::inscription_transfers))
          .route("/inscription_transfers_number/:number", get(Self::inscription_transfers_number))
          .route("/inscriptions_in_address/:address", get(Self::inscriptions_in_address))
          .route("/inscriptions_on_sat/:sat", get(Self::inscriptions_on_sat))
          .route("/sat_metadata/:sat", get(Self::sat_metadata))
          .layer(map_response(Self::set_header))
          .layer(
            TraceLayer::new_for_http()
              .make_span_with(DefaultMakeSpan::new().level(TraceLevel::INFO))
              .on_request(|req: &Request<Body>, _span: &Span| {
                tracing::event!(TraceLevel::INFO, "Started processing request {}", req.uri().path());
              })
              .on_response(|res: &Response<BoxBody>, latency: Duration, _span: &Span| {
                tracing::event!(TraceLevel::INFO, "Finished processing request latency={:?} status={:?}", latency, res.status());
              })
          )
          .with_state(server_config);

        let addr = SocketAddr::from(([127, 0, 0, 1], self.api_http_port.unwrap_or(81)));
        //tracing::debug!("listening on {}", addr);
        axum::Server::bind(&addr)
            .serve(app.into_make_service())
            .with_graceful_shutdown(Self::shutdown_signal())
            .await
            .unwrap();
      });
      println!("Vermilion server stopped");
    });
    return verm_server_thread;
  }

  pub(crate) fn run_ordinals_server(self, options: Options, index: Arc<Index>, handle: Handle) -> JoinHandle<()> {
    //1. Ordinals Server
    let server = server::Server {
      address: self.address,
      acme_domain: self.acme_domain,
      http_port: self.http_port,
      https_port: self.https_port,
      acme_cache: self.acme_cache,
      acme_contact: self.acme_contact,
      http: self.http,
      https: self.https,
      redirect_http_to_https: self.redirect_http_to_https,
    };
    let server_thread = thread::spawn(move || {
      let server_result = server.run(options, index, handle);
      match server_result {
        Ok(_) => {
          println!("Ordinals server stopped");
        },
        Err(err) => {
          println!("Ordinals server failed to start: {:?}", err);
        }
      }
    });
    return server_thread;
  }

  //Inscription Indexer Helper functions
  pub(crate) async fn upload_ordinal_content(client: &s3::Client, bucket_name: &str, inscription_id: InscriptionId, inscription: Inscription, head_check: bool) {
    let id = inscription_id.to_string();	
    let key = format!("content/{}", id);
    if head_check {
      let head_status = client	
        .head_object()	
        .bucket(bucket_name)	
        .key(key.clone())	
        .send()	
        .await;
      match head_status {	
        Ok(_) => {	
          log::debug!("Ordinal content already exists in S3: {}", id.clone());	
          return;	
        }	
        Err(error) => {	
          if error.to_string() == "service error" {
            let service_error = error.into_service_error();
            if service_error.to_string() != "NotFound" {
              println!("Error checking if ordinal {} exists in S3: {} - {:?} code: {:?}", id.clone(), service_error, service_error.message(), service_error.code());	
              return;	//error
            } else {
              log::trace!("Ordinal {} not found in S3, uploading", id.clone());
            }
          } else {
            println!("Error checking if ordinal {} exists in S3: {} - {:?}", id.clone(), error, error.message());	
            return; //error
          }
        }
      };
    }
    
    let body = Inscription::body(&inscription);	
    let bytes = match body {	
      Some(body) => body.to_vec(),	
      None => {	
        println!("No body found for inscription: {}, filling with empty body", inscription_id);	
        Vec::new()	
      }	
    };	
    let content_type = match Inscription::content_type(&inscription) {	
      Some(content_type) => content_type,	
      None => {	
        println!("No content type found for inscription: {}, filling with empty content type", inscription_id);	
        ""	
      }	
    };
    let put_status = client	
      .put_object()	
      .bucket(bucket_name)	
      .key(key)	
      .body(ByteStream::from(bytes))	
      .content_type(content_type)	
      .send()	
      .await;

    let _ret = match put_status {	
      Ok(put_status) => {	
        log::debug!("Uploaded ordinal content to S3: {}", id.clone());	
        put_status	
      }	
      Err(error) => {	
        println!("Error uploading ordinal {} to S3: {} - {:?}", id.clone(), error, error.message());	
        return;	
      }	
    };
  }

  fn is_bitmap_style(input: &str) -> bool {
    let pattern = r"^[a-zA-Z0-9]+[.][a-zA-Z0-9]+$";
    let re = regex::Regex::new(pattern).unwrap();
    re.is_match(input)
  }
  
  fn is_recursive(input: &str) -> bool {
    input.contains("/content")
  }

  fn is_maybe_json(input: &str) -> bool {  
      let first_char = input.chars().next().unwrap();
      let last_char = input.chars().last().unwrap();  
      first_char == '{' && last_char == '}'
  }

  pub(crate) fn extract_ordinal_metadata(index: Arc<Index>, inscription_id: InscriptionId, inscription: Inscription) -> Result<(Metadata, Option<SatMetadata>)> {
    let entry = index
      .get_inscription_entry(inscription_id)
      .unwrap()
      .unwrap();
    let satpoint = index
      .get_inscription_satpoint_by_id(inscription_id)
      .unwrap()
      .unwrap();
    let content_length = match inscription.content_length() {
      Some(content_length) => Some(content_length as i64),
      None => {
        println!("No content length found for inscription: {}, filling with 0", inscription_id);
        Some(0)
      }
    };
    let sat = match entry.sat {
      Some(sat) => Some(sat.n() as i64),
      None => {
        None
      }
    };
    let sha256 = match inscription.body() {
      Some(body) => {
        let hash = digest(body);
        Some(hash)
      },
      None => {
        None
      }
    };
    let text = match inscription.body() {
      Some(body) => {
        let text = String::from_utf8(body.to_vec());
        match text {
          Ok(text) => Some(text),
          Err(_) => None
        }
      },
      None => {
        None
      }
    };
    let is_json = match inscription.body() {
      Some(body) => {
        let json = serde_json::from_slice::<serde::de::IgnoredAny>(body);
        match json {
          Ok(_) => true,
          Err(_) => false
        }
      },
      None => {
        false
      }
    };
    let is_maybe_json = match text.clone() {
      Some(text) => Self::is_maybe_json(&text),
      None => false
    };
    let is_bitmap_style = match text.clone() {
      Some(text) => Self::is_bitmap_style(&text),
      None => false
    };
    let is_recursive = match text.clone() {
      Some(text) => Self::is_recursive(&text),
      None => false
    };
    let metadata = Metadata {
      id: inscription_id.to_string(),
      content_length: content_length,
      content_type: inscription.content_type().map(str::to_string),
      genesis_fee: entry.fee.try_into().unwrap(),
      genesis_height: entry.height.try_into().unwrap(),
      genesis_transaction: inscription_id.txid.to_string(),
      location: satpoint.to_string(),
      number: entry.inscription_number,
      sequence_number: Some(entry.sequence_number),
      offset: satpoint.offset.try_into().unwrap(),
      output_transaction: satpoint.outpoint.to_string(),
      sat: sat,
      timestamp: entry.timestamp.try_into().unwrap(),
      sha256: sha256,
      text: text,
      is_json: is_json,
      is_maybe_json: is_maybe_json,
      is_bitmap_style: Some(is_bitmap_style),
      is_recursive: Some(is_recursive)
    };
    let sat_metadata = match entry.sat {
      Some(sat) => {
        let sat_blocktime = index.block_time(sat.height())?;
        let sat_metadata = SatMetadata {
          sat: sat.0,
          decimal: sat.decimal().to_string(),
          degree: sat.degree().to_string(),
          name: sat.name(),
          block: sat.height().0,
          cycle: sat.cycle(),
          epoch: sat.epoch().0,
          period: sat.period(),
          offset: sat.third(),
          rarity: sat.rarity().to_string(),
          percentile: sat.percentile(),
          satpoint: satpoint.to_string(),
          timestamp: sat_blocktime.timestamp().timestamp()
        };
        Some(sat_metadata)
      },
      None => None
    };
    Ok((metadata, sat_metadata))
  }

  pub(crate) async fn create_metadata_table(pool: &mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
      r"CREATE TABLE IF NOT EXISTS ordinals (
          id varchar(80) not null primary key,
          content_length bigint,
          content_type text,
          genesis_fee bigint,
          genesis_height bigint,
          genesis_transaction text,
          location text,
          number bigint,
          sequence_number bigint unsigned,
          offset bigint,
          output_transaction text,
          sat bigint,
          timestamp bigint,
          sha256 varchar(64),
          text mediumtext,
          is_json boolean,
          is_maybe_json boolean,
          is_bitmap_style boolean,
          is_recursive boolean,
          INDEX index_id (id),
          INDEX index_number (number),
          INDEX index_sequence_number (sequence_number),
          INDEX index_block (genesis_height),
          INDEX index_sha256 (sha256),
          INDEX index_sat (sat)
      )").await.unwrap();
    Ok(())
  }

  pub(crate) async fn create_sat_table(pool: &mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
      r"CREATE TABLE IF NOT EXISTS sat (
        sat bigint not null primary key,
        sat_decimal text,
        degree text,
        name text,
        block bigint unsigned,
        cycle bigint unsigned,
        epoch bigint unsigned,
        period bigint unsigned,
        offset bigint unsigned,
        rarity varchar(10),
        percentile text,
        satpoint text,
        timestamp bigint,
        INDEX index_sat (sat),
        INDEX index_block (block),
        INDEX index_rarity (rarity)
      )").await.unwrap();
    Ok(())
  }

  pub(crate) async fn bulk_insert_metadata(pool: &mysql_async::Pool, metadata_vec: Vec<Metadata>) -> Result<(), Box<dyn std::error::Error + Send>> {
    let mut conn = pool.get_conn().await.unwrap();
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    let _exec = tx.exec_batch(
      r"INSERT INTO ordinals (id, content_length, content_type, genesis_fee, genesis_height, genesis_transaction, location, number, sequence_number, offset, output_transaction, sat, timestamp, sha256, text, is_json, is_maybe_json, is_bitmap_style, is_recursive)
        VALUES (:id, :content_length, :content_type, :genesis_fee, :genesis_height, :genesis_transaction, :location, :number, :sequence_number, :offset, :output_transaction, :sat, :timestamp, :sha256, :text, :is_json, :is_maybe_json, :is_bitmap_style, :is_recursive)
        ON DUPLICATE KEY UPDATE content_length=VALUES(content_length), content_type=VALUES(content_type), genesis_fee=VALUES(genesis_fee), genesis_height=VALUES(genesis_height), genesis_transaction=VALUES(genesis_transaction), 
        location=VALUES(location), number=VALUES(number), sequence_number=VALUES(sequence_number), offset=VALUES(offset), output_transaction=VALUES(output_transaction), sat=VALUES(sat), timestamp=VALUES(timestamp), sha256=VALUES(sha256), text=VALUES(text), 
        is_json=VALUES(is_json), is_maybe_json=VALUES(is_maybe_json), is_bitmap_style=VALUES(is_bitmap_style), is_recursive=VALUES(is_recursive)",
        metadata_vec.iter().map(|metadata| params! { 
          "id" => &metadata.id,
          "content_length" => &metadata.content_length,
          "content_type" => &metadata.content_type,
          "genesis_fee" => &metadata.genesis_fee,
          "genesis_height" => &metadata.genesis_height,
          "genesis_transaction" => &metadata.genesis_transaction,
          "location" => &metadata.location,
          "number" => &metadata.number,
          "sequence_number" => &metadata.sequence_number,
          "offset" => &metadata.offset,
          "output_transaction" => &metadata.output_transaction,
          "sat" => &metadata.sat,
          "timestamp" => &metadata.timestamp,
          "sha256" => &metadata.sha256,
          "text" => &metadata.text,
          "is_json" => &metadata.is_json,
          "is_maybe_json" => &metadata.is_maybe_json,
          "is_bitmap_style" => &metadata.is_bitmap_style,
          "is_recursive" => &metadata.is_recursive
      })
    ).await;
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error bulk inserting ordinal metadata: {}", error);
        Err(Box::new(error))
      }
    }
  }

  pub(crate) async fn bulk_insert_sat_metadata(pool: &mysql_async::Pool, metadata_vec: Vec<SatMetadata>) -> Result<(), Box<dyn std::error::Error + Send>> {
    let mut conn = pool.get_conn().await.unwrap();
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    let _exec = tx.exec_batch(
      r"INSERT INTO sat (sat, sat_decimal, degree, name, block, cycle, epoch, period, offset, rarity, percentile, satpoint, timestamp)
        VALUES (:sat, :sat_decimal, :degree, :name, :block, :cycle, :epoch, :period, :offset, :rarity, :percentile, :satpoint, :timestamp)
        ON DUPLICATE KEY UPDATE sat_decimal=VALUES(sat_decimal), degree=VALUES(degree), name=VALUES(name), block=VALUES(block), cycle=VALUES(cycle), epoch=VALUES(epoch), 
        period=VALUES(period), offset=VALUES(offset), rarity=VALUES(rarity), percentile=VALUES(percentile), satpoint=VALUES(satpoint), timestamp=VALUES(timestamp)",
        metadata_vec.iter().map(|metadata| params! {
          "sat" => &metadata.sat,
          "sat_decimal" => &metadata.decimal,
          "degree" => &metadata.degree,
          "name" => &metadata.name,
          "block" => &metadata.block,
          "cycle" => &metadata.cycle,
          "epoch" => &metadata.epoch,
          "period" => &metadata.period,
          "offset" => &metadata.offset,
          "rarity" => &metadata.rarity,
          "percentile" => &metadata.percentile,
          "satpoint" => &metadata.satpoint,
          "timestamp" => &metadata.timestamp
      })
    ).await;
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error bulk inserting ordinal sat metadata: {}", error);
        Err(Box::new(error))
      }
    }
  }

  pub(crate) async fn get_last_number(pool: &mysql_async::Pool) -> Result<u64, Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    let row = conn.query_iter("select min(previous) from (select number, Lag(number,1) over (order BY number) as previous from ordinals) a where number != previous+1")
      .await
      .unwrap()
      .next()
      .await
      .unwrap()
      .unwrap();
    let row = mysql_async::from_row::<Option<u64>>(row);
    let number = match row {
      Some(row) => {
        let number: u64 = row;
        number
      },
      None => {
        let row = conn.query_iter("select max(number) from ordinals")
          .await
          .unwrap()
          .next()
          .await
          .unwrap()
          .unwrap();
        let max = mysql_async::from_row::<Option<u64>>(row);
        match max {
          Some(max) => {
            let number: u64 = max;
            number
          },
          None => {
            0
          }
        }
      }
    };

    Ok(number)
  }

  pub(crate) async fn get_needed_sequence_numbers(status_vector: Arc<Mutex<Vec<SequenceNumberStatus>>>) -> Vec<u64> {
    let mut status_vector = status_vector.lock().await;
    let largest_number_in_vec = status_vector.iter().max_by_key(|status| status.sequence_number).unwrap().sequence_number;
    let mut needed_inscription_numbers: Vec<u64> = Vec::new();
    //Find start of needed numbers
    let mut pending_count=0;
    let mut unknown_count=0;
    let mut error_count=0;
    let mut not_found_count=0;
    let mut success_count=0;
    for status in status_vector.iter() {
      if status.status == "UNKNOWN" || status.status == "ERROR" || status.status == "NOT_FOUND" {
        needed_inscription_numbers.push(status.sequence_number);
      }
      if status.status == "PENDING" {
        pending_count = pending_count + 1;
      }
      if status.status == "UNKNOWN" {
        unknown_count = unknown_count + 1;
      }
      if status.status == "ERROR" {
        error_count = error_count + 1;
      }
      if status.status == "NOT_FOUND" || status.status == "NOT_FOUND_LOCKED" {
        not_found_count = not_found_count + 1;
      }
      if status.status == "SUCCESS" {
        success_count = success_count + 1;
      }
    }
    log::info!("Pending: {}, Unknown: {}, Error: {}, Not Found: {}, Success: {}", pending_count, unknown_count, error_count, not_found_count, success_count);
    //Fill in needed numbers
    let mut needed_length = needed_inscription_numbers.len();
    needed_inscription_numbers.sort();
    if needed_length < 1000 {
      let mut i = 0;
      while needed_length < 1000 {        
        i = i + 1;
        needed_inscription_numbers.push(largest_number_in_vec + i);
        needed_length = needed_inscription_numbers.len();
      }
    } else {
      needed_inscription_numbers = needed_inscription_numbers[0..1000].to_vec();
    }
    //Mark as pending
    for number in needed_inscription_numbers.clone() {
      match status_vector.iter_mut().find(|status| status.sequence_number == number) {
        Some(status) => {
          status.status = "PENDING".to_string();
        },
        None => {
          let status = SequenceNumberStatus{
            sequence_number: number,
            status: "PENDING".to_string(),
          };
          status_vector.push(status);
        }
      };
    }
    //Remove successfully processed numbers from vector
    status_vector.retain(|status| status.status != "SUCCESS");
    needed_inscription_numbers
  }

  pub(crate) async fn print_index_timings(timings: Arc<Mutex<Vec<IndexerTimings>>>, n_threads: u32) {
    let mut locked_timings = timings.lock().await;
    locked_timings.sort_by(|a, b| a.inscription_start.cmp(&b.inscription_start));
    //First get the relevant entries
    let mut relevant_timings: Vec<IndexerTimings> = Vec::new();
    let mut last = locked_timings.last().unwrap().inscription_start + 1000;
    for timing in locked_timings.iter().rev() {
      if timing.inscription_start == last - 1000 {
        relevant_timings.push(timing.clone());
        if relevant_timings.len() == n_threads as usize {
          break;
        }
      } else {
        relevant_timings = Vec::new();
        relevant_timings.push(timing.clone());
      }      
      last = timing.inscription_start;
    }
    if relevant_timings.len() < n_threads as usize {
      return;
    }    
    relevant_timings.sort_by(|a, b| a.inscription_start.cmp(&b.inscription_start));    
    let mut queueing_total = Duration::new(0,0);
    let mut acquire_permit_total = Duration::new(0,0);
    let mut get_numbers_total = Duration::new(0,0);
    let mut get_id_total = Duration::new(0,0);
    let mut get_inscription_total = Duration::new(0,0);
    let mut upload_content_total = Duration::new(0,0);
    let mut get_metadata_total = Duration::new(0,0);
    let mut retrieval_total = Duration::new(0,0);
    let mut insertion_total = Duration::new(0,0);
    let mut locking_total = Duration::new(0,0);
    let mut last_start = relevant_timings.first().unwrap().acquire_permit_start;
    for timing in relevant_timings.iter() {
      queueing_total = queueing_total + timing.acquire_permit_start.duration_since(last_start);
      acquire_permit_total = acquire_permit_total + timing.acquire_permit_end.duration_since(timing.acquire_permit_start);
      get_numbers_total = get_numbers_total + timing.get_numbers_end.duration_since(timing.get_numbers_start);
      get_id_total = get_id_total + timing.get_id_end.duration_since(timing.get_id_start);
      get_inscription_total = get_inscription_total + timing.get_inscription_end.duration_since(timing.get_inscription_start);
      upload_content_total = upload_content_total + timing.upload_content_end.duration_since(timing.upload_content_start);
      get_metadata_total = get_metadata_total + timing.get_metadata_end.duration_since(timing.get_metadata_start);
      retrieval_total = retrieval_total + timing.retrieval;
      insertion_total = insertion_total + timing.insertion;
      locking_total = locking_total + timing.locking;
      last_start = timing.acquire_permit_start;
    }
    let count = relevant_timings.last().unwrap().inscription_end - relevant_timings.first().unwrap().inscription_start+1;
    let total_time = relevant_timings.last().unwrap().get_metadata_end.duration_since(relevant_timings.first().unwrap().get_numbers_start);
    log::debug!("Inscriptions {}-{}", relevant_timings.first().unwrap().inscription_start, relevant_timings.last().unwrap().inscription_end);
    log::debug!("Total time: {:?}, avg per inscription: {:?}", total_time, total_time/count as u32);
    log::debug!("Queueing time avg per thread: {:?}", queueing_total/n_threads); //9 because the first one doesn't have a recorded queueing time
    log::debug!("Acquiring Permit time avg per thread: {:?}", acquire_permit_total/n_threads); //should be similar to queueing time
    log::debug!("Get numbers time avg per thread: {:?}", get_numbers_total/n_threads);
    log::debug!("Get id time avg per thread: {:?}", get_id_total/n_threads);
    log::debug!("Get inscription time avg per thread: {:?}", get_inscription_total/n_threads);
    log::debug!("Upload content time avg per thread: {:?}", upload_content_total/n_threads);
    log::debug!("Get metadata time avg per thread: {:?}", get_metadata_total/n_threads);
    log::debug!("--Retrieval time avg per thread: {:?}", retrieval_total/n_threads);
    log::debug!("--Insertion time avg per thread: {:?}", insertion_total/n_threads);
    log::debug!("--Locking time avg per thread: {:?}", locking_total/n_threads);

    //Remove printed timings
    let to_remove = BTreeSet::from_iter(relevant_timings);
    locked_timings.retain(|e| !to_remove.contains(e));

  }

  //Address Indexer Helper functions
  pub(crate) async fn create_transfers_table(pool: &mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
      r"CREATE TABLE IF NOT EXISTS transfers (
        id varchar(80) not null,
        block_number bigint not null,
        block_timestamp bigint,
        satpoint text,
        transaction text,
        address text,
        is_genesis boolean,
        PRIMARY KEY (`id`,`block_number`),
        INDEX index_id (id),
        INDEX index_block (block_number)
      )").await.unwrap();
    Ok(())
  }

  pub(crate) async fn bulk_insert_transfers(pool: &mysql_async::Pool, transfer_vec: Vec<Transfer>) -> Result<(), Box<dyn std::error::Error + Send>> {
    let mut conn = pool.get_conn().await.unwrap();
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    let _exec = tx.exec_batch(
      r"INSERT INTO transfers (id, block_number, block_timestamp, satpoint, transaction, address, is_genesis)
        VALUES (:id, :block_number, :block_timestamp, :satpoint, :transaction, :address, :is_genesis)
        ON DUPLICATE KEY UPDATE block_timestamp=VALUES(block_timestamp), satpoint=VALUES(satpoint), transaction=VALUES(transaction), address=VALUES(address), is_genesis=VALUES(is_genesis)",
        transfer_vec.iter().map(|transfer| params! { 
          "id" => &transfer.id,
          "block_number" => &transfer.block_number,
          "block_timestamp" => &transfer.block_timestamp,
          "satpoint" => &transfer.satpoint,
          "transaction" => &transfer.transaction,
          "address" => &transfer.address,
          "is_genesis" => &transfer.is_genesis
      })
    ).await;
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error bulk inserting ordinal transfers: {}", error);
        Err(Box::new(error))
      }
    }
  }

  pub(crate) async fn create_address_table(pool: &mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    conn.query_drop(
      r"CREATE TABLE IF NOT EXISTS addresses (
        id varchar(80) not null primary key,
        block_number bigint not null,
        block_timestamp bigint,
        satpoint text,
        transaction text,
        address varchar(100),
        is_genesis boolean,
        INDEX index_id (id),
        INDEX index_address (address)
      )").await.unwrap();
    Ok(())
  }

  pub(crate) async fn bulk_insert_addresses(pool: &mysql_async::Pool, transfer_vec: Vec<Transfer>) -> Result<(), Box<dyn std::error::Error + Send>> {
    let mut conn = pool.get_conn().await.unwrap();
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    let _exec = tx.exec_batch(
      r"INSERT INTO addresses (id, block_number, block_timestamp, satpoint, transaction, address, is_genesis)
        VALUES (:id, :block_number, :block_timestamp, :satpoint, :transaction, :address, :is_genesis)
        ON DUPLICATE KEY UPDATE block_number=VALUES(block_number), block_timestamp=VALUES(block_timestamp), satpoint=VALUES(satpoint), transaction=VALUES(transaction), address=VALUES(address), is_genesis=VALUES(is_genesis)",
        transfer_vec.iter().map(|transfer| params! { 
          "id" => &transfer.id,
          "block_number" => &transfer.block_number,
          "block_timestamp" => &transfer.block_timestamp,
          "satpoint" => &transfer.satpoint,
          "transaction" => &transfer.transaction,
          "address" => &transfer.address,
          "is_genesis" => &transfer.is_genesis
      })
    ).await;
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error bulk inserting ordinal addresses: {}", error);
        Err(Box::new(error))
      }
    }
  }

  pub(crate) async fn get_start_block(pool: &mysql_async::Pool) -> Result<u64, Box<dyn std::error::Error>> {
    let mut conn = pool.get_conn().await.unwrap();
    let row = conn.query_iter("select max(block_number) from transfers")
      .await
      .unwrap()
      .next()
      .await
      .unwrap()
      .unwrap();
    let row = mysql_async::from_row::<Option<i64>>(row);
    let block_number = match row {
      Some(row) => {
        let block_number: u64 = row.try_into().unwrap();
        block_number+1
      },
      None => {
        0
      }
    };
    Ok(block_number)
  }
  //Server api functions
  async fn root() -> &'static str {    
"One of the fastest ways to dox yourself as a cryptopleb is to ask \"what's the reason for the Bitcoin pump today.\"

Its path to $1m+ is preordained. On any given day it needs no reasons."
  }

  async fn home(State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let response = Self::get_ordinal_content(&server_config.s3client, &server_config.bucket_name, "6fb976ab49dcec017f1e201e84395983204ae1a7c2abf7ced0a85d692e442799i0".to_string()).await;
    let bytes = response.body.collect().await.unwrap().to_vec();
    let content_type = response.content_type.unwrap();
    (
        ([(axum::http::header::CONTENT_TYPE, content_type)]),
        bytes,
    )
  }

  async fn set_header<B>(response: Response<B>) -> Response<B> {
    //response.headers_mut().insert("cache-control", "public, max-age=31536000, immutable".parse().unwrap());
    response
  }

  async fn inscription(Path(inscription_id): Path<InscriptionId>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let response = Self::get_ordinal_content(&server_config.s3client, &server_config.bucket_name, inscription_id.to_string()).await;
    let bytes = response.body.collect().await.unwrap().to_vec();
    let content_type = response.content_type.unwrap();
    (
      ([(axum::http::header::CONTENT_TYPE, content_type),
        (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string())]),
      bytes,
    )
  }

  async fn inscription_number(Path(number): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let response = Self::get_ordinal_content_by_number(server_config.pool, &server_config.s3client, &server_config.bucket_name, number).await;
    let bytes = response.body.collect().await.unwrap().to_vec();
    let content_type = response.content_type.unwrap();
    (
      ([(axum::http::header::CONTENT_TYPE, content_type),
        (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string())]),
      bytes,
    )
  }

  async fn inscription_sha256(Path(sha256): Path<String>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let response = Self::get_ordinal_content_by_sha256(server_config.pool, &server_config.s3client, &server_config.bucket_name, sha256).await;
    let bytes = response.body.collect().await.unwrap().to_vec();
    let content_type = response.content_type.unwrap();
    (
      ([(axum::http::header::CONTENT_TYPE, content_type),
        (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable".to_string())]),
      bytes,
    )
  }

  async fn inscription_metadata(Path(inscription_id): Path<InscriptionId>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let metadata = Self::get_ordinal_metadata(server_config.pool, inscription_id.to_string()).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json"),
        (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")]),
      Json(metadata),
    )
  }

  async fn inscription_metadata_number(Path(number): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let metadata = Self::get_ordinal_metadata_by_number(server_config.pool, number).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json"),
        (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")]),
      Json(metadata),
    )
  }

  async fn inscription_editions(Path(inscription_id): Path<InscriptionId>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let editions = Self::get_matching_inscriptions(server_config.pool, inscription_id.to_string()).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(editions),
    )
  }

  async fn inscription_editions_number(Path(number): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let editions = Self::get_matching_inscriptions_by_number(server_config.pool, number).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(editions),
    )
  }

  async fn inscription_editions_sha256(Path(sha256): Path<String>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let editions = Self::get_matching_inscriptions_by_sha256(server_config.pool, sha256).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(editions),
    )
  }

  async fn inscriptions_in_block(Path(block): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let inscriptions = Self::get_inscriptions_within_block(server_config.pool, block).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json"),
      (axum::http::header::CACHE_CONTROL, "public, max-age=31536000, immutable")]),
      Json(inscriptions),
    )
  }

  async fn random_inscription(State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let inscription_number = Self::get_random_inscription(server_config.pool).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(inscription_number),
    )
  }

  async fn inscription_last_transfer(Path(inscription_id): Path<InscriptionId>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let transfer = Self::get_last_ordinal_transfer(server_config.pool, inscription_id.to_string()).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(transfer),
    )
  }

  async fn inscription_last_transfer_number(Path(number): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let transfer = Self::get_last_ordinal_transfer_by_number(server_config.pool, number).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(transfer),
    )
  }

  async fn inscription_transfers(Path(inscription_id): Path<InscriptionId>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let transfers = Self::get_ordinal_transfers(server_config.pool, inscription_id.to_string()).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(transfers),
    )
  }

  async fn inscription_transfers_number(Path(number): Path<i64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let transfers = Self::get_ordinal_transfers_by_number(server_config.pool, number).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(transfers),
    )
  }

  async fn inscriptions_in_address(Path(address): Path<String>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let inscriptions: Vec<TransferWithMetadata> = Self::get_inscriptions_by_address(server_config.pool, address).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(inscriptions),
    )
  }

  async fn inscriptions_on_sat(Path(sat): Path<u64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let inscriptions: Vec<Metadata> = Self::get_inscriptions_on_sat(server_config.pool, sat).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(inscriptions),
    )
  }

  async fn sat_metadata(Path(sat): Path<u64>, State(server_config): State<ApiServerConfig>) -> impl axum::response::IntoResponse {
    let sat_metadata = Self::get_sat_metadata(server_config.pool, sat).await;
    (
      ([(axum::http::header::CONTENT_TYPE, "application/json")]),
      Json(sat_metadata),
    )
  }

  async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("expect tokio signal ctrl-c");
  }

  //DB functions
  async fn get_ordinal_content(client: &s3::Client, bucket_name: &str, inscription_id: String) -> GetObjectOutput {
    let key = format!("content/{}", inscription_id);
    let content = client
      .get_object()
      .bucket(bucket_name)
      .key(key)
      .send()
      .await
      .unwrap();
    content
  }

  async fn get_ordinal_content_by_number(pool: mysql_async::Pool, client: &s3::Client, bucket_name: &str, number: i64) -> GetObjectOutput {
    let mut conn = Self::get_conn(pool).await;
    let inscription_id: String = conn.exec_first(
      "SELECT id FROM ordinals WHERE number=:number LIMIT 1", 
      params! {
        "number" => number
      }
    )
    .await
    .unwrap()
    .unwrap();

    let content = Self::get_ordinal_content(client, bucket_name, inscription_id).await;
    content
  }

  async fn get_ordinal_content_by_sha256(pool: mysql_async::Pool, client: &s3::Client, bucket_name: &str, sha256: String) -> GetObjectOutput {
    let mut conn = Self::get_conn(pool).await;
    let inscription_id: String = conn.exec_first(
      "SELECT id FROM ordinals WHERE sha256=:sha256 LIMIT 1", 
      params! {
        "sha256" => sha256
      }
    )
    .await
    .unwrap()
    .unwrap();

    let content = Self::get_ordinal_content(client, bucket_name, inscription_id).await;
    content
  }

  async fn get_ordinal_metadata(pool: mysql_async::Pool, inscription_id: String) -> Metadata {
    let mut conn = Self::get_conn(pool).await;
    let result = conn.exec_map(
      "SELECT * FROM ordinals WHERE id=:id LIMIT 1", 
      params! {
        "id" => inscription_id
      },
      |mut row: mysql_async::Row| Metadata {
        id: row.get("id").unwrap(),
        content_length: row.take("content_length").unwrap(),
        content_type: row.take("content_type").unwrap(), 
        genesis_fee: row.get("genesis_fee").unwrap(),
        genesis_height: row.get("genesis_height").unwrap(),
        genesis_transaction: row.get("genesis_transaction").unwrap(),
        location: row.get("location").unwrap(),
        number: row.get("number").unwrap(),
        sequence_number: row.take("sequence_number").unwrap(),
        offset: row.get("offset").unwrap(),
        output_transaction: row.get("output_transaction").unwrap(),
        sat: row.take("sat").unwrap(),
        timestamp: row.get("timestamp").unwrap(),
        sha256: row.take("sha256").unwrap(),
        text: row.take("text").unwrap(),
        is_json: row.get("is_json").unwrap(),
        is_maybe_json: row.get("is_maybe_json").unwrap(),
        is_bitmap_style: row.take("is_bitmap_style").unwrap(),
        is_recursive: row.take("is_recursive").unwrap()
      }
    );
    let result = result.await.unwrap().pop().unwrap();
    result
  }

  async fn get_ordinal_metadata_by_number(pool: mysql_async::Pool, number: i64) -> Metadata {
    let mut conn = Self::get_conn(pool).await;
    let result = conn.exec_map(
      "SELECT * FROM ordinals WHERE number=:number LIMIT 1", 
      params! {
        "number" => number
      },
      |mut row: mysql_async::Row| Metadata {
        id: row.get("id").unwrap(),
        content_length: row.take("content_length").unwrap(),
        content_type: row.take("content_type").unwrap(), 
        genesis_fee: row.get("genesis_fee").unwrap(),
        genesis_height: row.get("genesis_height").unwrap(),
        genesis_transaction: row.get("genesis_transaction").unwrap(),
        location: row.get("location").unwrap(),
        number: row.get("number").unwrap(),
        sequence_number: row.get("sequence_number").unwrap(),
        offset: row.get("offset").unwrap(),
        output_transaction: row.get("output_transaction").unwrap(),
        sat: row.take("sat").unwrap(),
        timestamp: row.get("timestamp").unwrap(),
        sha256: row.take("sha256").unwrap(),
        text: row.take("text").unwrap(),
        is_json: row.get("is_json").unwrap(),
        is_maybe_json: row.get("is_maybe_json").unwrap(),
        is_bitmap_style: row.get("is_bitmap_style").unwrap(),
        is_recursive: row.get("is_recursive").unwrap()
      }
    );
    let result = result.await.unwrap().pop().unwrap();
    result    
  }

  async fn get_matching_inscriptions(pool: mysql_async::Pool, inscription_id: String) -> Vec<InscriptionNumberEdition> {
    let mut conn = Self::get_conn(pool).await;
    let editions = conn.exec_map(
      "with a as (select sha256 from editions where id = :id) select id, number, edition from editions,a where editions.sha256=a.sha256 order by edition asc",
      params! {
        "id" => inscription_id
      },
      |row: mysql_async::Row| InscriptionNumberEdition {
        id: row.get("id").unwrap(),
        number: row.get("number").unwrap(),
        edition: row.get("edition").unwrap()
      }
    ).await.unwrap();
    editions
  }

  async fn get_matching_inscriptions_by_number(pool: mysql_async::Pool, number: i64) -> Vec<InscriptionNumberEdition> {
    let mut conn = Self::get_conn(pool).await;
    let editions = conn.exec_map(
      "with a as (select sha256 from editions where number = :number) select id, number, edition from editions,a where editions.sha256=a.sha256 order by edition asc", 
      params! {
        "number" => number
      },
      |row: mysql_async::Row| InscriptionNumberEdition {
        id: row.get("id").unwrap(),
        number: row.get("number").unwrap(),
        edition: row.get("edition").unwrap()
      }
    ).await.unwrap();
    editions
  }

  async fn get_matching_inscriptions_by_sha256(pool: mysql_async::Pool, sha256: String) -> Vec<InscriptionNumberEdition> {
    let mut conn = Self::get_conn(pool).await;
    let editions = conn.exec_map(
      "select id, number, edition from editions where sha256=:sha256 order by edition asc",
      params! {
        "sha256" => sha256
      },
      |row: mysql_async::Row| InscriptionNumberEdition {
        id: row.get("id").unwrap(),
        number: row.get("number").unwrap(),
        edition: row.get("edition").unwrap()
      }
    ).await.unwrap();
    editions
  }

  async fn get_inscriptions_within_block(pool: mysql_async::Pool, block: i64) -> Vec<InscriptionMetadataForBlock> {
    let mut conn = Self::get_conn(pool).await;
    let inscriptions = conn.exec_map(
      "SELECT id, content_length, content_type, genesis_fee, genesis_height, number, timestamp FROM ordinals WHERE genesis_height=:block", 
      params! {
        "block" => block
      },
      |mut row: mysql_async::Row| InscriptionMetadataForBlock {
        id: row.get("id").unwrap(),
        content_length: row.take("content_length").unwrap(),
        content_type: row.take("content_type").unwrap(), 
        genesis_fee: row.get("genesis_fee").unwrap(),
        genesis_height: row.get("genesis_height").unwrap(),
        number: row.get("number").unwrap(),
        timestamp: row.get("timestamp").unwrap()
      }
    ).await.unwrap();
    inscriptions
  }
  
  async fn get_random_inscription(pool: mysql_async::Pool) -> RandomInscriptionNumber {
    let mut conn = Self::get_conn(pool).await;
    let mut rng = rand::rngs::StdRng::from_entropy();
    let random_float = rng.gen::<f64>();
    let random_inscription_number: RandomInscriptionNumber = conn.exec_map(
      "SELECT first_number FROM weights where band_end>:random_float limit 1;", 
      params! {
        "random_float" => random_float
      },
      |row: mysql_async::Row| RandomInscriptionNumber {
        number: row.get("first_number").unwrap()
      }
    ).await
    .unwrap()
    .pop()
    .unwrap();
    random_inscription_number
  }

  async fn get_conn(pool: mysql_async::Pool) -> mysql_async::Conn {
    let conn: mysql_async::Conn = pool.get_conn().await.unwrap();
    conn
  }

  async fn get_last_ordinal_transfer(pool: mysql_async::Pool, inscription_id: String) -> Transfer {
    let mut conn = Self::get_conn(pool).await;
    let transfer = conn.exec_map(
      "select * from transfers where id=:id order by block_number desc limit 1", 
      params! {
        "id" => inscription_id
      },
      |row: mysql_async::Row| Transfer {
        id: row.get("id").unwrap(),
        block_number: row.get("block_number").unwrap(),
        block_timestamp: row.get("block_timestamp").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        transaction: row.get("transaction").unwrap(),
        address: row.get("address").unwrap(),
        is_genesis: row.get("is_genesis").unwrap()
      }
    ).await.unwrap().pop().unwrap();
    transfer
  }

  async fn get_last_ordinal_transfer_by_number(pool: mysql_async::Pool, number: i64) -> Transfer {
    let mut conn = Self::get_conn(pool).await;
    let transfer = conn.exec_map(
      "with a as (Select id from ordinals where number=:number) select b.* from transfers b, a where a.id=b.id order by block_number desc limit 1", 
      params! {
        "number" => number
      },
      |row: mysql_async::Row| Transfer {
        id: row.get("id").unwrap(),
        block_number: row.get("block_number").unwrap(),
        block_timestamp: row.get("block_timestamp").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        transaction: row.get("transaction").unwrap(),
        address: row.get("address").unwrap(),
        is_genesis: row.get("is_genesis").unwrap()
      }
    ).await.unwrap().pop().unwrap();
    transfer
  }

  async fn get_ordinal_transfers(pool: mysql_async::Pool, inscription_id: String) -> Vec<Transfer> {
    let mut conn = Self::get_conn(pool).await;
    let transfers = conn.exec_map(
      "select * from transfers where id=:id order by block_number asc", 
      params! {
        "id" => inscription_id
      },
      |row: mysql_async::Row| Transfer {
        id: row.get("id").unwrap(),
        block_number: row.get("block_number").unwrap(),
        block_timestamp: row.get("block_timestamp").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        transaction: row.get("transaction").unwrap(),
        address: row.get("address").unwrap(),
        is_genesis: row.get("is_genesis").unwrap()
      }
    ).await.unwrap();
    transfers
  }

  async fn get_ordinal_transfers_by_number(pool: mysql_async::Pool, number: i64) -> Vec<Transfer> {
    let mut conn = Self::get_conn(pool).await;
    let transfers = conn.exec_map(
      "with a as (Select id from ordinals where number=:number) select b.* from transfers b, a where a.id=b.id order by block_number desc", 
      params! {
        "number" => number
      },
      |row: mysql_async::Row| Transfer {
        id: row.get("id").unwrap(),
        block_number: row.get("block_number").unwrap(),
        block_timestamp: row.get("block_timestamp").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        transaction: row.get("transaction").unwrap(),
        address: row.get("address").unwrap(),
        is_genesis: row.get("is_genesis").unwrap()
      }
    ).await.unwrap();
    transfers
  }

  async fn get_inscriptions_by_address(pool: mysql_async::Pool, address: String) -> Vec<TransferWithMetadata> {
    let mut conn = Self::get_conn(pool).await;
    let transfers = conn.exec_map(
      "select a.*, o.* from addresses a left join ordinals o on a.id=o.id where a.address=:address",
      params! {
        "address" => address
      },
      |mut row: mysql_async::Row| TransferWithMetadata {
        id: row.get("id").unwrap(),
        block_number: row.get("block_number").unwrap(),
        block_timestamp: row.get("block_timestamp").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        transaction: row.get("transaction").unwrap(),
        address: row.get("address").unwrap(),
        is_genesis: row.get("is_genesis").unwrap(),
        content_length: row.take("content_length").unwrap(),
        content_type: row.take("content_type").unwrap(),
        genesis_fee: row.get("genesis_fee").unwrap(),
        genesis_height: row.get("genesis_height").unwrap(),
        genesis_transaction: row.get("genesis_transaction").unwrap(),
        location: row.get("location").unwrap(),
        number: row.get("number").unwrap(),
        sequence_number: row.take("sequence_number").unwrap(),
        offset: row.get("offset").unwrap(),
        output_transaction: row.get("output_transaction").unwrap(),
        sat: row.take("sat").unwrap(),
        timestamp: row.get("timestamp").unwrap(),
        sha256: row.take("sha256").unwrap(),
        text: row.take("text").unwrap(),
        is_json: row.get("is_json").unwrap(),
        is_maybe_json: row.get("is_maybe_json").unwrap(),
        is_bitmap_style: row.take("is_bitmap_style").unwrap(),
        is_recursive: row.take("is_recursive").unwrap()
      }
    ).await.unwrap();
    transfers
  }

  async fn get_inscriptions_on_sat(pool: mysql_async::Pool, sat: u64) -> Vec<Metadata> {
    let mut conn = Self::get_conn(pool).await;
    let result = conn.exec_map(
      "SELECT * FROM ordinals WHERE sat=:sat", 
      params! {
        "sat" => sat
      },
      |mut row: mysql_async::Row| Metadata {
        id: row.get("id").unwrap(),
        content_length: row.take("content_length").unwrap(),
        content_type: row.take("content_type").unwrap(), 
        genesis_fee: row.get("genesis_fee").unwrap(),
        genesis_height: row.get("genesis_height").unwrap(),
        genesis_transaction: row.get("genesis_transaction").unwrap(),
        location: row.get("location").unwrap(),
        number: row.get("number").unwrap(),
        sequence_number: row.get("sequence_number").unwrap(),
        offset: row.get("offset").unwrap(),
        output_transaction: row.get("output_transaction").unwrap(),
        sat: row.take("sat").unwrap(),
        timestamp: row.get("timestamp").unwrap(),
        sha256: row.take("sha256").unwrap(),
        text: row.take("text").unwrap(),
        is_json: row.get("is_json").unwrap(),
        is_maybe_json: row.get("is_maybe_json").unwrap(),
        is_bitmap_style: row.get("is_bitmap_style").unwrap(),
        is_recursive: row.get("is_recursive").unwrap()
      }
    );
    let result = result.await.unwrap();
    result
  }

  async fn get_sat_metadata(pool: mysql_async::Pool, sat: u64) -> SatMetadata {
    let mut conn = Self::get_conn(pool).await;
    let result = conn.exec_map(
      "SELECT * FROM sat WHERE sat=:sat", 
      params! {
        "sat" => sat
      },
      |row: mysql_async::Row| SatMetadata {
        sat: row.get("sat").unwrap(),
        decimal: row.get("sat_decimal").unwrap(),
        degree: row.get("degree").unwrap(),
        name: row.get("name").unwrap(),
        block: row.get("block").unwrap(),
        cycle: row.get("cycle").unwrap(),
        epoch: row.get("epoch").unwrap(),
        period: row.get("period").unwrap(),
        offset: row.get("offset").unwrap(),
        rarity: row.get("rarity").unwrap(),
        percentile: row.get("percentile").unwrap(),
        satpoint: row.get("satpoint").unwrap(),
        timestamp: row.get("timestamp").unwrap()
      }
    );
    let result = result.await.unwrap().pop().unwrap();
    result
  }

  async fn create_edition_procedure(pool: mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Self::get_conn(pool).await;
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    tx.query_drop(r"DROP PROCEDURE IF EXISTS update_editions").await.unwrap();
    tx.query_drop(
      r#"CREATE PROCEDURE update_editions()
      BEGIN
      IF "editions" NOT IN (SELECT table_name FROM information_schema.tables) THEN
      INSERT into proc_log(proc_name, step_name, ts) values ("EDITIONS", "START_CREATE", now());
      CREATE TABLE editions as select id, number, sha256, row_number() OVER(PARTITION BY sha256 ORDER BY number asc) as edition, count(number) OVER(PARTITION BY sha256) as total from ordinals;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("EDITIONS", "FINISH_CREATE", now(), found_rows());
      CREATE INDEX idx_id ON editions (id);
      CREATE INDEX idx_number ON editions (number);
      CREATE INDEX idx_sha256 ON editions (sha256);
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("EDITIONS", "FINISH_INDEX", now(), found_rows());
      ELSE
      DROP TABLE IF EXISTS editions_new;
      INSERT into proc_log(proc_name, step_name, ts) values ("EDITIONS", "START_CREATE_NEW", now());
      CREATE TABLE editions_new as select id, number, sha256, row_number() OVER(PARTITION BY sha256 ORDER BY number asc) as edition, count(number) OVER(PARTITION BY sha256) as total from ordinals;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("EDITIONS", "FINISH_CREATE_NEW", now(), found_rows());
      CREATE INDEX idx_id ON editions_new (id);
      CREATE INDEX idx_number ON editions_new (number);
      CREATE INDEX idx_sha256 ON editions_new (sha256);
      RENAME TABLE editions to editions_old, editions_new to editions;
      DROP TABLE IF EXISTS editions_old;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("EDITIONS", "FINISH_INDEX_NEW", now(), found_rows());
      END IF;
      END;"#).await.unwrap();
    tx.query_drop(r"DROP EVENT IF EXISTS editions_event").await.unwrap();
    tx.query_drop(r"CREATE EVENT editions_event ON SCHEDULE EVERY 24 HOUR STARTS FROM_UNIXTIME(CEILING(UNIX_TIMESTAMP(CURTIME())/86400)*86400) DO CALL update_editions()").await.unwrap();
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error creating editions table stored procedure: {}", error);
        Err(Box::new(error))
      }
    }
  }

  async fn create_weights_procedure(pool: mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Self::get_conn(pool).await;
    let mut tx = conn.start_transaction(TxOpts::default()).await.unwrap();
    tx.query_drop(r"DROP PROCEDURE IF EXISTS update_weights").await.unwrap();
    tx.query_drop(
      r#"CREATE PROCEDURE update_weights()
      BEGIN
      IF "weights" NOT IN (SELECT table_name FROM information_schema.tables) THEN
      INSERT into proc_log(proc_name, step_name, ts) values ("WEIGHTS", "START_CREATE", now());
      CREATE TABLE weights as
      select b.*, sum(b.weight) OVER(order by b.first_number)/sum(b.weight) OVER() as band_end, coalesce(sum(b.weight) OVER(order by b.first_number ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING),0)/sum(b.weight) OVER() as band_start from (
        select a.*, (10-log(10,a.first_number+1))*total_fee*(1-is_json)*(1-is_bitmap_style)*(1-is_maybe_json) as weight from (
          select sha256, 
                 min(number) as first_number, 
                 sum(genesis_fee) as total_fee, 
                 max(content_length) as content_length, 
                 count(*) as count, 
                 max(is_json) as is_json,
                 max(is_bitmap_style) as is_bitmap_style,
                 max(is_maybe_json) as is_maybe_json
          from ordinals group by sha256
        ) a
      ) b;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("WEIGHTS", "FINISH_CREATE", now(), found_rows());
      CREATE INDEX idx_band_start ON weights (band_start);
      CREATE INDEX idx_band_end ON weights (band_end);
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("WEIGHTS", "FINISH_INDEX", now(), found_rows());
      ELSE
      INSERT into proc_log(proc_name, step_name, ts) values ("WEIGHTS", "START_CREATE_NEW", now());
      DROP TABLE IF EXISTS weights_new;
      CREATE TABLE weights_new as
      select b.*, sum(b.weight) OVER(order by b.first_number)/sum(b.weight) OVER() as band_end, coalesce(sum(b.weight) OVER(order by b.first_number ROWS BETWEEN UNBOUNDED PRECEDING AND 1 PRECEDING),0)/sum(b.weight) OVER() as band_start from (
        select a.*, (10-log(10,a.first_number+1))*total_fee*(1-is_json)*(1-is_bitmap_style)*(1-is_maybe_json) as weight from (
          select sha256, 
                 min(number) as first_number, 
                 sum(genesis_fee) as total_fee, 
                 max(content_length) as content_length, 
                 count(*) as count, 
                 max(is_json) as is_json,
                 max(is_bitmap_style) as is_bitmap_style,
                 max(is_maybe_json) as is_maybe_json
          from ordinals group by sha256
        ) a
      ) b;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("WEIGHTS", "FINISH_CREATE_NEW", now(), found_rows());
      CREATE INDEX idx_band_start ON weights_new (band_start);
      CREATE INDEX idx_band_end ON weights_new (band_end);
      RENAME TABLE weights to weights_old, weights_new to weights;
      DROP TABLE IF EXISTS weights_old;
      INSERT into proc_log(proc_name, step_name, ts, rows_returned) values ("WEIGHTS", "FINISH_INDEX_NEW", now(), found_rows());
      END IF;
      END;"#).await.unwrap();
    tx.query_drop(r"DROP EVENT IF EXISTS weights_event").await.unwrap();
    tx.query_drop(r"CREATE EVENT weights_event ON SCHEDULE EVERY 24 HOUR STARTS FROM_UNIXTIME(CEILING(UNIX_TIMESTAMP(CURTIME())/86400)*86400 + 43200) DO CALL update_weights()").await.unwrap();
    let result = tx.commit().await;
    match result {
      Ok(_) => Ok(()),
      Err(error) => {
        println!("Error creating weights table stored procedure: {}", error);
        Err(Box::new(error))
      }
    }
  }

  async fn create_procedure_log(pool: mysql_async::Pool) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Self::get_conn(pool).await;
    conn.query_drop(
      r"CREATE TABLE IF NOT EXISTS proc_log (
        id int unsigned auto_increment primary key,
        proc_name varchar(40),
        step_name varchar(40),
        ts timestamp,
        rows_returned int
      )").await.unwrap();
    Ok(())
  }

}