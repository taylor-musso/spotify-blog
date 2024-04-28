# spotify-blog

### usage

- to run:
  - `RUST_LOG=info cargo run` inside of spotify-blog
- commands:
  - `list songs`
    - lists all songs
  - `create song <title>|<artist>|<lyrics>|<explicit>`
    - creates a song with specified title/artist/lyrics & if it is explicit or not
    - ex: `create song do not touch|misamo|lyrics|false`
  - `publish song <id>`
    - publishes song with specified id
    - ex: `publish song 4`
   

### proposal
I am planning on implementing a blog app for songs, where users can share song data with each other via a p2p network. You can add songs, make them public, list songs (either local ones or of discovered peers), and list peers. 
