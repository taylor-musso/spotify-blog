use libp2p::{
    core::upgrade,
    floodsub::{Floodsub, FloodsubEvent, Topic},
    futures::StreamExt,
    identity,
    mdns::{Mdns, MdnsEvent},
    mplex,
    noise::{Keypair, NoiseConfig, X25519Spec},
    swarm::{NetworkBehaviourEventProcess, Swarm, SwarmBuilder},
    tcp::TokioTcpConfig,
    NetworkBehaviour, PeerId, Transport,
};
use log::{error, info};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::{fs, io::AsyncBufReadExt, sync::mpsc};
use dialoguer::{Input, Confirm};

const STORAGE_FILE_PATH: &str = "./songs.json";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync + 'static>>;
type Songs = Vec<Song>;

static KEYS: Lazy<identity::Keypair> = Lazy::new(|| identity::Keypair::generate_ed25519());
static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));
static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("songs"));

#[derive(Debug, Serialize, Deserialize)]
struct Song {
    id: usize,
    title: String,
    artist: String,
    lyrics: String,
    explicit: String,
    public: bool,
}

#[derive(Debug, Serialize, Deserialize)]
enum ListMode {
    ALL,
    One(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct ListRequest {
    mode: ListMode,
}

#[derive(Debug, Serialize, Deserialize)]
struct ListResponse {
    mode: ListMode,
    data: Songs,
    receiver: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatMessage {
    msg: String,
}

enum EventType {
    Response(ListResponse),
    Input(String),
}

#[derive(NetworkBehaviour)]
struct SongBehaviour {
    floodsub: Floodsub,
    mdns: Mdns,
    #[behaviour(ignore)]
    song_response_sender: mpsc::UnboundedSender<ListResponse>,
    #[behaviour(ignore)]
    #[allow(dead_code)]
    chat_message_sender: mpsc::UnboundedSender<ChatMessage>,
    
}

impl NetworkBehaviourEventProcess<FloodsubEvent> for SongBehaviour {
    fn inject_event(&mut self, event: FloodsubEvent) {
        match event {
            FloodsubEvent::Message(msg) => {
                if let Ok(resp) = serde_json::from_slice::<ListResponse>(&msg.data) {
                    if resp.receiver == PEER_ID.to_string() {
                    println!("Response from {}:\n\n\n", msg.source);
                    println!("Id      Title                  Artist               Lyrics");
                    println!("======= ====================== ==================== =======================\n");
                    resp.data.iter().for_each(|r| print_song(r));
                    }
                } else if let Ok(req) = serde_json::from_slice::<ListRequest>(&msg.data) {
                    match req.mode {
                        ListMode::ALL => {
                            info!("Received ALL req: {:?} from {:?}", req, msg.source);
                            respond_with_public_songs(
                                self.song_response_sender.clone(),
                                msg.source.to_string(),
                            );
                        }
                        ListMode::One(ref peer_id) => {
                            if peer_id == &PEER_ID.to_string() {
                                info!("Received req: {:?} from {:?}", req, msg.source);
                                respond_with_public_songs(
                                    self.song_response_sender.clone(),
                                    msg.source.to_string(),
                                );
                            }
                        }
                    }
                } else if let Ok(chat_msg) = serde_json::from_slice::<ChatMessage>(&msg.data) {
                    let p = msg.source.to_string();
                    let p = p[p.len() - 4..].to_string();
                    println!("From [{}]: {}", p, chat_msg.msg);
                }
            }
            _ => (),
        }
    }
}

fn respond_with_public_songs(sender: mpsc::UnboundedSender<ListResponse>, receiver: String) {
    tokio::spawn(async move {
        match read_local_songs().await {
            Ok(songs) => {
                let resp = ListResponse {
                    mode: ListMode::ALL,
                    receiver,
                    data: songs.into_iter().filter(|r| r.public).collect(),
                };
                if let Err(e) = sender.send(resp) {
                    error!("error sending response via channel, {}", e);
                }
            }
            Err(e) => error!("error fetching local songs to answer ALL request, {}", e),
        }
    });
}

impl NetworkBehaviourEventProcess<MdnsEvent> for SongBehaviour {
    fn inject_event(&mut self, event: MdnsEvent) {
        match event {
            MdnsEvent::Discovered(discovered_list) => {
                for (peer, _addr) in discovered_list {
                    self.floodsub.add_node_to_partial_view(peer);
                }
            }
            MdnsEvent::Expired(expired_list) => {
                for (peer, _addr) in expired_list {
                    if !self.mdns.has_node(&peer) {
                        self.floodsub.remove_node_from_partial_view(&peer);
                    }
                }
            }
        }
    }
}

async fn create_new_song(title: &str, artist: &str, lyrics: &str, explicit: &str) -> Result<()> {
    let mut local_songs = read_local_songs().await?;
    let new_id = match local_songs.iter().max_by_key(|r| r.id) {
        Some(v) => v.id + 1,
        None => 0,
    };
    local_songs.push(Song {
        id: new_id,
        title: title.to_owned(),
        artist: artist.to_owned(),
        lyrics: lyrics.to_owned(),
        explicit: explicit.to_owned(),
        public: false,
    });
    write_local_songs(&local_songs).await?;

    println!("\n\nCreated Song");
    println!("================");
    let new_song = Song {
        id: new_id,
        title: title.to_owned(),
        artist: artist.to_owned(),
        lyrics: lyrics.to_owned(),
        explicit: explicit.to_owned(),
        public: false,
    };
    print_song(&new_song);
    Ok(())
}

async fn delete_song(id: usize) -> Result<()> {
    let mut local_songs = read_local_songs().await?;
    if let Some(index) = local_songs.iter().position(|song| song.id == id) {
        local_songs.remove(index);
        write_local_songs(&local_songs).await?;
    } else {
        println!("Song with id {} not found.", id);
    }
    Ok(())
}

async fn publish_song(id: usize) -> Result<()> {
    let mut local_songs = read_local_songs().await?;
    local_songs
        .iter_mut()
        .filter(|r| r.id == id)
        .for_each(|r| r.public = true);
    write_local_songs(&local_songs).await?;
    Ok(())
}

async fn private_song(id: usize) -> Result<()> {
    let mut local_songs = read_local_songs().await?;
    local_songs
        .iter_mut()
        .filter(|r| r.id == id)
        .for_each(|r| r.public = false);
    write_local_songs(&local_songs).await?;
    Ok(())
}

async fn read_local_songs() -> Result<Songs> {
    let content = fs::read(STORAGE_FILE_PATH).await?;
    let result = serde_json::from_slice(&content)?;
    Ok(result)
}

async fn write_local_songs(songs: &Songs) -> Result<()> {
    let json = serde_json::to_string(&songs)?;
    fs::write(STORAGE_FILE_PATH, &json).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    println!("\n\n\n##############################################");
    println!("###                                        ###");
    println!("###        Welcome to Spotify Blog!        ###");
    println!("###                                        ###");
    println!("##############################################\n\n");
    println!("Available Commands");
    println!("===================");
    println!("[other]");
    println!("list peers           - Prints a list of all peers on the network");
    println!("chat                 - Send a message to all peers\n");
    println!("[songs]");
    println!("list songs           - Prints a list of all local songs");
    println!("list songs all       - Prints a list of all songs from peers");
    println!("list songs <peer id> - Prints a list of all songs from specified peer");
    println!("create song          - Creates a new song");
    println!("delete song <id>     - Deletes the song at the specified id");
    println!("publish song <id>    - Publishes the song at the specified id");
    println!("private song <id>    - Privates the song at the specified id");
    
    println!("\n\n\nYour peer id is: {}\n\n\n", PEER_ID.clone());

    let (song_response_sender, mut response_rcv) = mpsc::unbounded_channel();
    let (chat_message_sender, mut chat_rcv) = mpsc::unbounded_channel();

    let auth_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&KEYS)
        .expect("can create auth keys");

    let transp = TokioTcpConfig::new()
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
        .multiplex(mplex::MplexConfig::new())
        .boxed();

    let mut behaviour = SongBehaviour {
        floodsub: Floodsub::new(PEER_ID.clone()),
        mdns: Mdns::new(Default::default())
            .await
            .expect("can create mdns"),
        song_response_sender,
        chat_message_sender,
    };

    behaviour.floodsub.subscribe(TOPIC.clone());

    let mut swarm = SwarmBuilder::new(transp, behaviour, PEER_ID.clone())
        .executor(Box::new(|fut| {
            tokio::spawn(fut);
        }))
        .build();

    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin()).lines();

    Swarm::listen_on(
        &mut swarm,
        "/ip4/0.0.0.0/tcp/0"
            .parse()
            .expect("can get a local socket"),
    )
    .expect("swarm can be started");

    loop {
        let evt = {
            tokio::select! {
                line = stdin.next_line() => Some(EventType::Input(line.expect("can get line").expect("can read line from stdin"))),
                response = response_rcv.recv() => Some(EventType::Response(response.expect("response exists"))),
                chat_msg = chat_rcv.recv() => Some(EventType::Input(chat_msg.expect("chat message exists").msg)), 
                event = swarm.select_next_some() =>  {
                    info!("Unhandled Swarm Event: {:?}", event);
                    None
                },
            }
        };

        if let Some(event) = evt {
            match event {
                EventType::Response(resp) => {
                    let json = serde_json::to_string(&resp).expect("can jsonify response");
                    swarm
                        .behaviour_mut()
                        .floodsub
                        .publish(TOPIC.clone(), json.as_bytes());
                }
                EventType::Input(line) => match line.trim() {
                    "list peers" => handle_list_peers(&mut swarm).await,
                    "chat" => handle_chat(&mut swarm).await,
                    cmd if cmd.starts_with("list songs") => handle_list_songs(cmd, &mut swarm).await,
                    cmd if cmd.starts_with("create song") => handle_create_song(cmd).await,
                    cmd if cmd.starts_with("delete song") => handle_delete_song(cmd).await,
                    cmd if cmd.starts_with("publish song") => handle_publish_song(cmd).await,
                    cmd if cmd.starts_with("private song") => handle_private_song(cmd).await,
                    _ => error!("unknown command"),
                },
            }

        }
    }
}

async fn handle_list_peers(swarm: &mut Swarm<SongBehaviour>) {
    println!("\n\n\n\n######################");
    println!("#  Discovered Peers  #");
    println!("######################\n\n");
    let nodes = swarm.behaviour().mdns.discovered_nodes();
    let mut unique_peers = HashSet::new();
    for peer in nodes {
        unique_peers.insert(peer);
    }
    unique_peers.iter().for_each(|p| println!("{}", p));
}

async fn handle_list_songs(cmd: &str, swarm: &mut Swarm<SongBehaviour>) {
    let rest = cmd.strip_prefix("list songs ");
    match rest {
        Some("all") => {
            println!("\n\n\n\n################");
            println!("#  Peer Songs  #");
            println!("################\n");
            let req = ListRequest {
                mode: ListMode::ALL,
            };
            let json = serde_json::to_string(&req).expect("can jsonify request");
            swarm
                .behaviour_mut()
                .floodsub
                .publish(TOPIC.clone(), json.as_bytes());
        }
        Some(songs_peer_id) => {
            println!("\n\n\n\n################");
            println!("#  Peer Songs  #");
            println!("################\n");
            let req = ListRequest {
                mode: ListMode::One(songs_peer_id.to_owned()),
            };
            let json = serde_json::to_string(&req).expect("can jsonify request");
            swarm
                .behaviour_mut()
                .floodsub
                .publish(TOPIC.clone(), json.as_bytes());
        }
        None => {
            match read_local_songs().await {
                Ok(v) => {
                    info!("Local Songs ({})", v.len());
                    println!("\n\n\n\n#################");
                    println!("#  Local Songs  #");
                    println!("#################\n\n\n");
                    println!("Id      Title                  Artist               Lyrics");
                    println!("======= ====================== ==================== =======================\n");
                    v.iter().for_each(|song| print_song(song));
                }
                Err(e) => error!("error fetching local songs: {}", e),
            };
        }
    };
}

async fn handle_create_song(cmd: &str) {
    if let Some(_rest) = cmd.strip_prefix("create song") {

        let input_title = Input::<String>::new().with_prompt("Title").interact().unwrap();
        let input_artist = Input::<String>::new().with_prompt("Artist").interact().unwrap();
        let input_lyrics = Input::<String>::new().with_prompt("Lyrics").interact().unwrap();
        let input_explicit = Confirm::new().with_prompt("Explicit").default(true).interact().unwrap().to_string();

        if input_title.is_empty() || input_artist.is_empty() || input_lyrics.is_empty() {
            println!("too few arguments -- need title, artist, lyrics, and explicit");
        } else {
            if let Err(e) = create_new_song(&input_title, &input_artist, &input_lyrics, &input_explicit).await {
                error!("error creating song: {}", e);
            }
        }
    }
}

async fn handle_publish_song(cmd: &str) {
    if let Some(rest) = cmd.strip_prefix("publish song") {
        match rest.trim().parse::<usize>() {
            Ok(id) => {
                if let Err(e) = publish_song(id).await {
                    println!("error publishing song with id {}, {}", id, e)
                } else {
                    println!("Published song with id: {}", id);
                }
            }
            Err(e) => error!("invalid id: {}, {}", rest.trim(), e),
        };
    }
}

async fn handle_private_song(cmd: &str) {
    if let Some(rest) = cmd.strip_prefix("private song") {
        match rest.trim().parse::<usize>() {
            Ok(id) => {
                if let Err(e) = private_song(id).await {
                    println!("error privating song with id {}, {}", id, e)
                } else {
                    println!("Privated song with id: {}", id);
                }
            }
            Err(e) => error!("invalid id: {}, {}", rest.trim(), e),
        };
    }
}

async fn handle_delete_song(cmd: &str) {
    if let Some(rest) = cmd.strip_prefix("delete song") {
        match rest.trim().parse::<usize>() {
            Ok(id) => {
                if let Err(e) = delete_song(id).await {
                    println!("Error deleting song with id {}: {}", id, e);
                } else {
                    println!("Deleted song with id: {}", id);
                }
            }
            Err(e) => println!("Invalid id: {}, {}", rest.trim(), e),
        };
    }
}


async fn handle_chat(swarm: &mut Swarm<SongBehaviour>) {
    let input_msg = Input::<String>::new().with_prompt("Message").interact().unwrap();
    let chat_msg = ChatMessage { msg: input_msg };
    let json = serde_json::to_string(&chat_msg).expect("can jsonify chat message");
    swarm
        .behaviour_mut()
        .floodsub
        .publish(TOPIC.clone(), json.as_bytes()); 
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        format!("{:<1$}", s, max_len)
    }
}

fn print_song(song: &Song) {
    let mut title = song.title.clone();
    title = title.trim_start().to_string();
                        
    let truth_value = matches!(&*song.explicit, "true" | "t" | "1" | "True" | "yes" | "Yes" | "y" | "Y" | "T");
    if truth_value {
        title += " 🅴";
    }
                        
    println!("{:7} {:<22} {:<20} {:<24}", 
        format!("[{}]", song.id), 
        format!("{:22}", truncate_string(&title, 20)), 
        format!("{:20}", truncate_string(&song.artist, 16)), 
        format!("{:24}", truncate_string(&song.lyrics, 24))
    );
}