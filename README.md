# spotify-blog

### usage

- to run:
  - `cargo run` inside of spotify-blog
- commands:
  - `list peers`
    -  lists all peers
  - `list songs`
    - lists all songs
  - `chat`
    - prompts you to input a message to send to other peers
  - `create song`
    - starts prompting you for artist/title/etc to create a new song
  - `delete song <id>`
    - deletes song with specified id
  - `publish song <id>`
    - publishes song with specified id
  - `private song <id>`
    - privates song with specified id
  - `lyrics <id>`
    - prints the full lyrics to the song with specified id 
   

### proposal
I am planning on implementing a blog app for songs, where users can share song data with each other via a p2p network. You can add songs, make them public, list songs (either local ones or of discovered peers), and list peers. 

For the final project, I plan to change the user interface by improving the CLI prompts (replacing the info logs with regular print statements that are formatted in a more legible way). I also plan to implement deleting songs, making songs private, viewing the full lyrics of a specified song (when displayed normally they will be shortened to make it easier to read everything else), and chatting between users (messages instead of song data). 
