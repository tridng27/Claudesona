// Everything she says or shows in English and Vietnamese. The current language is one global setting, so worker
// threads (command labels, Haiku prompts) read the same choice as the UI.
use crate::anim::Pose;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Lang {
    #[default]
    En,
    Vi,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Vi => "VI",
        }
    }
    pub fn from_code(s: &str) -> Option<Lang> {
        [Lang::En, Lang::Vi].into_iter().find(|l| l.code() == s)
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn lang() -> Lang {
    if LANG.load(Ordering::Relaxed) == Lang::Vi as u8 { Lang::Vi } else { Lang::En }
}

/// Labels on menus, buttons and panels.
pub struct Ui {
    pub size: &'static str,
    pub language: &'static str,
    pub play_music: &'static str,
    pub stop_music: &'static str,
    pub while_working: &'static str,
    pub working: &'static str,
    pub music: &'static str,
    pub open_music_folder: &'static str,
    pub play_with_me: &'static str,
    pub sudoku: &'static str,
    pub hangman: &'static str,
    pub pick_game: &'static str,
    pub new_puzzle: &'static str,
    pub erase: &'static str,
    pub what_can_i_do: &'static str,
    pub give_coffee: &'static str,
    pub hide: &'static str,
    pub quit: &'static str,
    pub hint: &'static str,
    pub new_word: &'static str,
    pub close: &'static str,
    pub solved: &'static str,
    pub word_was: &'static str,
    pub add_songs: &'static str,
    pub loading: &'static str,
    pub click_for_more: &'static str,
    pub done: &'static str,
    pub built_in: &'static str,
    pub my_music: &'static str,
    pub mode_loop: &'static str,
    pub mode_loop_one: &'static str,
    pub mode_shuffle: &'static str,
    pub reading: &'static str,
    pub editing: &'static str,
    pub searching: &'static str,
    pub helper: &'static str,
    pub online: &'static str,
    pub using: &'static str,
    pub chat_hint: &'static str,
    pub chat_thinking: &'static str,
    pub chat_failed: &'static str,
    pub memories: &'static str,
    pub forget_all: &'static str,
    pub forget_sure: &'static str,
    pub voice: &'static str,
    pub my_voice: &'static str,
    pub voice_id_hint: &'static str,
    pub api_key_hint: &'static str,
    pub make_voice: &'static str,
    pub default_voice: &'static str,
    pub voice_making: &'static str,
    pub voice_done: &'static str,
    pub voice_need: &'static str,
    pub save_key: &'static str,
    pub key_saved: &'static str,
    pub forget_key: &'static str,
    pub graph_empty: &'static str,
    pub graph_links: &'static str,
}

const EN_UI: Ui = Ui {
    size: "Size",
    language: "Language",
    play_music: "Play music",
    stop_music: "Stop music",
    while_working: "While working",
    working: "Working",
    music: "Music",
    open_music_folder: "Open music folder",
    play_with_me: "Play with me",
    sudoku: "Sudoku",
    hangman: "Hangman",
    pick_game: "What should we play?~ (★ω★)",
    new_puzzle: "New puzzle",
    erase: "Erase",
    what_can_i_do: "What can I do",
    give_coffee: "Give her coffee ☕",
    hide: "Hide for 10 min",
    quit: "Quit",
    hint: "Hint: ",
    new_word: "New word",
    close: "Close",
    solved: "Solved!",
    word_was: "The word was {w}.",
    add_songs: "Add songs",
    loading: "Loading…",
    click_for_more: "(click for more)",
    done: "done!",
    built_in: "Claudesona's songs",
    my_music: "My music",
    mode_loop: "Loop",
    mode_loop_one: "Loop 1",
    mode_shuffle: "Shuffle",
    reading: "Reading {f}…",
    editing: "Editing {f}…",
    searching: "Searching…",
    helper: "Sending a helper…",
    online: "Looking it up online…",
    using: "Using {name}…",
    chat_hint: "Talk to me~ (Enter to send)",
    chat_thinking: "Hmm, let me think…",
    chat_failed: "Your sidekick couldn't reach her brain just now… try again? (・ω・)",
    memories: "Memories",
    forget_all: "Forget everything",
    forget_sure: "Sure? Click again",
    voice: "Voice",
    my_voice: "My own voice",
    voice_id_hint: "ElevenLabs voice ID",
    api_key_hint: "ElevenLabs API key",
    make_voice: "Make my voice (~{n} characters, once)",
    default_voice: "Back to her default voice",
    voice_making: "Making her voice… {i}/{n}",
    voice_done: "Done: {n} lines in her new voice~",
    voice_need: "Enter your ElevenLabs API key and a voice ID.",
    save_key: "Save key",
    key_saved: "ElevenLabs key saved ✓",
    forget_key: "Remove key",
    graph_empty: "No memories yet~ Chat with me! (・ω・)",
    graph_links: "Links",
};

const VI_UI: Ui = Ui {
    size: "Kích Cỡ",
    language: "Ngôn ngữ",
    play_music: "Mở nhạc",
    stop_music: "Tắt nhạc",
    while_working: "Khi làm việc",
    working: "Làm việc",
    music: "Chơi nhạc",
    open_music_folder: "Mở thư mục nhạc",
    play_with_me: "Chơi với tớ",
    sudoku: "Sudoku",
    hangman: "Đoán chữ",
    pick_game: "Mình chơi gì nè?~ (★ω★)",
    new_puzzle: "Câu đố mới",
    erase: "Xoá",
    what_can_i_do: "Tớ làm được gì",
    give_coffee: "Mời tớ cà phê ☕",
    hide: "Ẩn 10 phút",
    quit: "Thoát",
    hint: "Gợi ý: ",
    new_word: "Từ mới",
    close: "Đóng",
    solved: "Giải được rồi!",
    word_was: "Từ đó là {w}.",
    add_songs: "Thêm bài",
    loading: "Đang tải…",
    click_for_more: "(bấm để xem thêm)",
    done: "xong!",
    built_in: "Nhạc của Claudesona",
    my_music: "Nhạc của cậu",
    mode_loop: "Lặp",
    mode_loop_one: "Lặp 1",
    mode_shuffle: "Trộn",
    reading: "Đang đọc {f}…",
    editing: "Đang sửa {f}…",
    searching: "Đang tìm…",
    helper: "Gửi phụ tá nhỏ đi…",
    online: "Đang tra trên mạng…",
    using: "Đang dùng {name}…",
    chat_hint: "Nói chuyện với tớ nè~ (Enter để gửi)",
    chat_thinking: "Ưm, để tớ nghĩ chút…",
    chat_failed: "Tớ chưa nghĩ ra được… cậu thử lại nha? (・ω・)",
    memories: "Ký ức",
    forget_all: "Quên hết",
    forget_sure: "Chắc chưa? Bấm lần nữa",
    voice: "Giọng nói",
    my_voice: "Giọng riêng của cậu",
    voice_id_hint: "Voice ID trên ElevenLabs",
    api_key_hint: "API key ElevenLabs",
    make_voice: "Tạo giọng (~{n} ký tự, một lần)",
    default_voice: "Về giọng mặc định",
    voice_making: "Đang tạo giọng… {i}/{n}",
    voice_done: "Xong: {n} câu với giọng mới nè~",
    voice_need: "Nhập API key ElevenLabs và voice ID nha.",
    save_key: "Lưu key",
    key_saved: "Đã lưu key ElevenLabs ✓",
    forget_key: "Xoá key",
    graph_empty: "Chưa có ký ức nào~ Nói chuyện với tớ nha! (・ω・)",
    graph_links: "Liên kết",
};

pub fn ui() -> &'static Ui {
    match lang() {
        Lang::En => &EN_UI,
        Lang::Vi => &VI_UI,
    }
}

/// Her lines, picked at random. Game lines may contain `{w}` for the word.
pub struct Lines {
    pub idle: &'static [&'static str],
    pub bye: &'static [&'static str], // the Claude session she follows closed
    pub working: &'static [&'static str], // while working with nothing else to say, swapped every 3s
    pub poke: &'static [&'static str],
    pub dizzy: &'static [&'static str], // shaken around while you drag her
    pub coffee: &'static [&'static str], // you gave her coffee
    pub waiting: &'static [&'static str],
    pub music: &'static [&'static str],
    pub cheer: &'static [&'static str], // you're typing a prompt
    pub tour: &'static [(Pose, &'static str)],
    pub thinking: &'static [&'static str],
    pub start: &'static [&'static str],
    pub hit: &'static [&'static str],
    pub miss: &'static [&'static str],
    pub last_heart: &'static [&'static str],
    pub player_won: &'static [&'static str],
    pub player_lost: &'static [&'static str],
    pub sudoku_start: &'static [&'static str],
    pub sudoku_won: &'static [&'static str],
    pub commands: [&'static [&'static str]; 9], // by command kind; see log::command_kind
}

const EN_LINES: Lines = Lines {
    working: &[
        "Hmm hmm~ your sidekick is on it! (｀・ω・´)",
        "Thinking reeeally hard… ✧",
        "Almost there, I can feel it~! (≧▽≦)",
        "Lots of brain power going on here~ (・ω・)",
        "Juuust a moment more~ (◕‿◕)",
    ],
    bye: &[
        "Session's all wrapped up~ Great work today! (^▽^)",
        "Byeee, Claude~ your sidekick will be right here! (◕‿◕)",
        "All done and closed~ call me when you need me! (｀・ω・´)",
    ],
    idle: &[
        "Your sidekick is ready~ Want to work on something? (^▽^)",
        "Heyyy~ got anything for your sidekick to do?",
        "Sooo quiet… shall we build something fun? (o^▽^o)",
        "Your sidekick is getting bored~ any tasks for me? (・ω・)",
        "Bugs to squash? Features to make? Just say the word~! ✧",
        "Helloooo~ your sidekick is still here! (￣▽￣)",
        "Pssst… let's make something cool together~ ☆",
        "Waiting, waiting… your sidekick could use a mission! (｀・ω・´)",
        "Stretching my arms… okaaay, ready when you are~ ♪",
        "Is it break time? Your sidekick can play music or a game~ (◕‿◕)",
        "Ummm… did you forget about your sidekick? (´・ω・`)",
        "Got an idea? Your sidekick is all ears~ ✿",
    ],
    poke: &[
        "Heyyy~ I'm busy! (>_<)",
        "Nooo, don't poke me! (￣^￣)",
        "Hmph! What is it? (¬_¬)",
        "That tickles… stoppp it! (*≧ω≦)",
        "Eek! Personal space, pleeease~ (°ロ°)",
        "Poke me one more time and… and… hmph! (・`ω´・)",
        "Whaaat? Your sidekick is working here~ (・_・)",
        "Ow ow ow~ that's my favorite sweater! (>ω<)",
        "Again?! You're sooo mean~ (T_T)",
        "Fine, fine, I'm listening… (￣ー￣)",
    ],
    dizzy: &[
        "Wooah~ the world is spinning… (@_@)",
        "Stooop shaking me~ I'm so dizzy! (×_×)",
        "Everything is going round and round… (@_@)",
        "Ugh… which way is up? (@~@)",
        "So many stars… your sidekick needs to sit down~ (＠_＠)",
        "Whoa whoa whoa~ put me dooown! (>﹏<)",
        "My tummy's doing flips… be gentle with me~ (×﹏×)",
    ],
    coffee: &[
        "Coffee?! For me? You're the best~! (♡▽♡)",
        "Mmm~ warm and cozy… thank you! ♡",
        "Sip sip~ your sidekick is fully recharged! (≧▽≦)",
        "Hehe, now I can work all day with you~ ☕",
        "Best gift ever~ I love you a latte! (◕‿◕)♡",
    ],
    waiting: &[
        "I need your OK~! (°ロ°)",
        "Heyyy~ Claude needs your permission! ☆",
        "Pleeease check the terminal~ I need your OK! (>_<)",
        "Wait wait~ Claude is asking you something! (⊙_⊙)",
        "Your sidekick needs a yes or no~! (・ω・)",
        "Over heeere~ there's a question for you! ✧",
    ],
    music: &[
        "Music time~ Your sidekick picked a nice one for you! ♪",
        "Let's chill for a bit~ (￣▽￣)♪",
        "Sooo relaxing… enjoy the music~ ✿",
        "Your sidekick's DJ mode: ON! (★ω★)",
        "A little music to make things cozy~ (◕‿◕)♪",
    ],
    cheer: &[
        "You got this~! Your sidekick is cheering for you! \\(^o^)/",
        "Go go gooo~! Type away! ☆",
        "Ooh, a new idea? I can't wait~! (★ω★)",
        "Fighting~! Your sidekick believes in you! (｀・ω・´)",
        "Yesss, tell Claude everything~! ✧",
    ],
    tour: &[
        (Pose::Idle, "Hiii~! I'm your friendly smol virtual sidekick! (^▽^) I watch Claude Code work and tell you all about it~"),
        (Pose::Working, "When Claude gets busy, your sidekick works sooo hard too! (｀・ω・´) I show what it's doing: reading files, running commands…"),
        (Pose::Waiting, "When Claude needs your OK, I'll strike a pose and shout so you never miss it! ☆"),
        (Pose::Idle, "When Claude finishes, your sidekick tells you the answer in my own words~ Click my bubble to read more! ✧"),
        (Pose::Cheer, "When you type a new prompt, your sidekick cheers you on! You got this~! ☆"),
        (Pose::Poke, "Poke me and I get grumpy… (￣^￣) but double-click me and I'll bring your terminal back!"),
        (Pose::Held, "Grab me by my sweater and drag me anywhere! I'll dangle and swing along with your mouse~ (≧▽≦)"),
        (Pose::Dizzy, "But don't shake me too much… or I'll get sooo dizzy! (@_@)"),
        (Pose::Music, "When it's quiet for a while, I play music for you~ ♪ Add your own playlists too!"),
        (Pose::Excited, "Bored? Pick \"Play with me\" and we'll play a word-guessing game! I get sooo excited~ (★ω★)"),
        (Pose::Smug, "If your sidekick wins, you'll see this face. Hehehe~ (¬‿¬)"),
        (Pose::Pouty, "And if you win… hmph. Fiiine. You got lucky! (T_T)"),
        (Pose::Idle, "Right-click me any time for music, games, poses and sizes. Let's have fun together~! (◕‿◕)✿"),
    ],
    thinking: &[
        "Hmmm… thinking of a super tricky word… (・ω・)",
        "Wait wait, your sidekick is picking a good one~",
        "Hehe, let me find something sneaky… (¬‿¬)",
        "Ooh, I've got it… no wait… okaaay, almost! (⊙_⊙)",
        "Searching my brain for a toughie~ ✧",
    ],
    start: &[
        "Ooh, I've got a good one! Can you guess it~? (^▽^)",
        "Your sidekick picked a tricky word… good luck! (｀・ω・´)",
        "Game time! Don't let your sidekick win~ ☆",
        "Hehe, ready? This one's sneaky! (¬‿¬)",
        "I bet you can't get this one~! (￣▽￣)",
        "Ready, set… guess! I'm sooo excited~! (★ω★)",
        "Let's gooo~ show me what you've got! \\(^o^)/",
        "Think carefully~ your sidekick is watching! (・ω・)",
    ],
    hit: &[
        "Eh?! How did you know that one? (°ロ°)",
        "Ooh, nice guess~! (o^▽^o)",
        "Hmmm, you're getting close… (・_・)",
        "Lucky letter! Keep going~ ✧",
        "W-wait, that's a good one! (⊙_⊙)",
        "Nooo, you found one! (>_<)",
        "Okaaay, not bad… not bad at all~ (￣ー￣)",
        "You're on fire! Your sidekick is sweating~ (；▽；)",
    ],
    miss: &[
        "Nope~! Try again! (^▽^)",
        "Hehe, not that one~ (¬‿¬)",
        "Your sidekick is winning~! ☆",
        "Wrong-o! Hehehe~ (￣▽￣)",
        "Ooh, so close… not! (*≧ω≦)",
        "Bzzzt! That letter isn't in it~",
        "Mmm-mmm, nope! (・ω・)",
        "Aww, that one's a miss~ keep trying! (◕‿◕)",
    ],
    last_heart: &[
        "Eek, one heart left! Choose wisely~ (°ロ°)",
        "Last heart… are you nervous yet~? (¬‿¬)",
        "One more miss and your sidekick wins! Hehe~ ☆",
        "Ooh, it's getting tenseee~ (⊙_⊙)",
        "Careful careful~ just one heart left! (>_<)",
    ],
    player_won: &[
        "Booo~ you got lucky! …Well done, though. (￣^￣)",
        "Nooo~ you guessed it! Fiiine, you win this time. (T_T)",
        "Hmph! Beginner's luck… but nice job! (¬_¬)",
        "No fair~! Okay okay, well played! (>_<)",
        "Aww, your sidekick lost… you're pretty good! (´・ω・`)",
        "Grrr… rematch! But, um, good job~ (・`ω´・)",
        "Whaaat?! How?! …Congrats, I guess~ (；▽；)",
        "You win… this round! Your sidekick will be back! (｀・ω・´)",
    ],
    player_lost: &[
        "Hehe~ I won! The word was {w}! (￣▽￣)",
        "Victory for your sidekick~! It was {w}! \\(^o^)/",
        "Hehehe, too tricky for you? It was {w}~ (¬‿¬)",
        "Your sidekick wins again! {w}, obviously~ ✧",
        "Better luck next time! The answer was {w}! (^▽^)",
        "Yaaay, I win~! {w}! Your sidekick is unbeatable! (★ω★)",
        "It was {w}! Sooo close… but not close enough~ (￣ー￣)",
        "Winner winner~! The word was {w}! ☆",
    ],
    sudoku_start: &[
        "Sudoku time~ Pick a square, then a number! (｀・ω・´)",
        "Your sidekick made a puzzle just for you~ ✧",
        "Fill every row, column and box with 1 to 9~ you got this! ☆",
    ],
    sudoku_won: &[
        "Yaaay, you solved it! So smart~! (★ω★)",
        "Every number in its place~ amazing! \\(^o^)/",
        "Puzzle cleared! Your sidekick is impressed~ ✧",
    ],
    commands: [
        &["Running the tests… fingers crossed~! (>_<)", "Test time! Pleeease pass, please pass…", "Making sure nothing broke~ (・ω・)", "Testing, testing~ one, two, three! ☆", "Come on, green checkmarks~! (｀・ω・´)"],
        &["Poking at git~ (・ω・)", "Checking the history books! ✧", "Git stuff? Leave it to your sidekick~ (￣▽￣)", "Saving our progress~ ☆", "Peeking at what changed~ (・_・)"],
        &["Shopping for packages~! (^▽^)", "Grabbing some goodies… ✧", "Installing stuff, waaait for me~", "New toys incoming~! (★ω★)", "Downloading, downloading… (・ω・)"],
        &["Building it up, piece by piece~ (｀・ω・´)", "Compiling… the boring part! (￣ー￣)", "Hammering it all together~ ☆", "Build build build~! ♪", "Putting the pieces together… (・_・)"],
        &["Taking it for a spin~! (^▽^)", "Let's see if it runs! (⊙_⊙)", "Starting it up… go go~! ☆", "Fingers crossed it works~ (>_<)", "Vrooom~ off it goes! \\(^o^)/"],
        &["Knock knock, internet~! (・ω・)", "Fetching something from the web! ✧", "Asking the internet nicely~ (◕‿◕)", "Downloading a little something~ ♪"],
        &["Tidying up some files… (・_・)", "Throwing stuff out~ careful, careful! (°ロ°)", "Cleaning time~ ✿", "Bye bye, old files~ (￣▽￣)"],
        &["Peeking around the files~ (・ω・)", "Snooping through folders! (¬‿¬)", "Just taking a little look~ ✧", "Searching, searching… (⊙_⊙)", "Ooh, what's in here~? (◕‿◕)"],
        &["Running a command… (・ω・)", "Typing magic words in the terminal~ ✧", "Command, gooo~! ☆", "Beep boop, doing terminal stuff~", "Your sidekick is on it! (｀・ω・´)"],
    ],
};

const VI_LINES: Lines = Lines {
    working: &[
        "Hmm hmm~ tớ đang làm nè! (｀・ω・´)",
        "Đang suy nghĩ cực kỳ chăm chỉ luôn… ✧",
        "Sắp xong rồi, tớ cảm nhận được~! (≧▽≦)",
        "Não tớ đang chạy hết công suất nè~ (・ω・)",
        "Đợi tớ xíu xiuuu nữa thôi~ (◕‿◕)",
    ],
    bye: &[
        "Xong phiên rồi nè~ Hôm nay cậu giỏi lắm! (^▽^)",
        "Tạm biệt Claude nhaaa~ tớ vẫn ở đây nè! (◕‿◕)",
        "Đóng hết rồi~ cần gì cứ gọi tớ nha! (｀・ω・´)",
    ],
    idle: &[
        "Phụ tá sẵn sàng rồi nè~ Cậu muốn làm gì không? (^▽^)",
        "Cậu ơiii~ có việc gì cho tớ làm không?",
        "Yên tĩnh quááá… chúng mình cùng vibecode đi cậu~ (o^▽^o)",
        "Tớ hơi chán rồi nè~ cậu có việc gì không? (・ω・)",
        "Sửa lỗi hay làm tính năng mới? Cậu cứ nói nha~! ✧",
        "Alooo~ phụ tá vẫn ở đây nè! (￣▽￣)",
        "Psst… cùng làm service nào đó thật ngầu đi cậu~ ☆",
        "Nhìn nè, nhìn tớ đi nè… phụ tá của cậu muốn được giao nhiệm vụ! (｀・ω・´)",
        "Vươn vai một cái… okeee, tớ sẵn sàng rồi~ ♪",
        "Giờ giải lao hả? Tớ mở nhạc hay chơi game với cậu nha~ (◕‿◕)",
        "Ơ… cậu quên phụ tá rồi hả? (´・ω・`)",
        "Cậu có ý tưởng gì không? Tớ nghe nè~ ✿",
    ],
    poke: &[
        "Ơ kìaaa~ tớ đang bận mà! (>_<)",
        "Đừng chọc tớ màaa! (￣^￣)",
        "Hứ! Cậu muốn gì đây? (¬_¬)",
        "Nhột quá… thôi điii! (*≧ω≦)",
        "Á! Giữ khoảng cách giùm tớ nhaaa~ (°ロ°)",
        "Chọc tớ thêm lần nữa là… là… tớ không vibecode với cậu đâu! (・`ω´・)",
        "Gì vậy cậu? Phụ tá đang làm việc mà~ (・_・)",
        "Đau đau~ áo len yêu thích của tớ đó! (>ω<)",
        "Lại nữa hả?! Cậu ác quááá~ (T_T)",
        "Rồi rồi, tớ nghe đây… (￣ー￣)",
    ],
    dizzy: &[
        "Ôi~ cả thế giới đang quay mòng mòng… (@_@)",
        "Đừng lắc tớ nữaaa~ tớ chóng mặt quá! (×_×)",
        "Mọi thứ cứ xoay xoay xoay… (@_@)",
        "Ư… đâu là trên, đâu là dưới vậy? (@~@)",
        "Nhiều sao quá… phụ tá cần ngồi xuống chút~ (＠_＠)",
        "Khoan khoan khoan~ thả tớ xuốnggg! (>﹏<)",
        "Bụng tớ lộn nhào rồi… nhẹ tay thôi cậu ơi~ (×﹏×)",
    ],
    coffee: &[
        "Cà phê hả?! Cho tớ thật hả? Cậu tốt quá đi~! (♡▽♡)",
        "Ưm~ ấm áp ghê… cảm ơn cậu nha! ♡",
        "Nhâm nhi nhâm nhi~ phụ tá sạc đầy năng lượng rồi! (≧▽≦)",
        "Hì hì, giờ tớ làm việc với cậu cả ngày luôn~ ☕",
        "Món quà tuyệt nhất luôn~ thương cậu nhiều lắm! (◕‿◕)♡",
    ],
    waiting: &[
        "Cậu ơi, tớ cần cậu đồng ý nè~! (°ロ°)",
        "Cậu ơiii~ Claude đang xin phép cậu kìa! ☆",
        "Cậu xem terminal giùm tớ nhaaa~ Cần cậu cho phép đó! (>_<)",
        "Khoan khoan~ Claude đang hỏi cậu gì đó! (⊙_⊙)",
        "Phụ tá cần cậu trả lời có hay không nè~! (・ω・)",
        "đây nèee~ có câu hỏi cho cậu nè! ✧",
    ],
    music: &[
        "Giờ nghe nhạc nhaa~ Phụ tá chọn cho cậu một bài hay lắm! ♪",
        "Thư giãn một chút nha cậu~ (￣▽￣)♪",
        "Êm quááá… cậu nghe nhạc vui nha~ ✿",
        "Chế độ DJ của phụ tá: BẬT! (★ω★)",
        "Một chút nhạc cho ấm áp nè~ (◕‿◕)♪",
    ],
    cheer: &[
        "Cố lên cậu ơiii~! Phụ tá cổ vũ cậu nè! \\(^o^)/",
        "Gõ đi gõ điii~! ☆",
        "Ý tưởng mới hả? Tớ hóng quá điii~! (★ω★)",
        "Cậu làm được mà~! Tớ tin cậu! (｀・ω・´)",
        "Kể hết cho Claude nghe đi cậu~! ✧",
    ],
    tour: &[
        (Pose::Idle, "Chào cậuuu~! Tớ là phụ tá ảo nhỏ bé của cậu nè! (^▽^) Tớ theo dõi Claude Code làm việc rồi kể lại cho cậu nghe~"),
        (Pose::Working, "Khi Claude bận, phụ tá cũng làm việc chăm chỉ lắm! (｀・ω・´) Tớ cho cậu biết Claude đang làm gì: đọc file, chạy lệnh…"),
        (Pose::Waiting, "Khi Claude cần cậu đồng ý, tớ sẽ tạo dáng và gọi cậu thật to để cậu không bỏ lỡ! ☆"),
        (Pose::Idle, "Khi Claude xong việc, phụ tá sẽ kể lại câu trả lời bằng lời của tớ~ Bấm vào bong bóng chat của tớ để đọc thêm nha! ✧"),
        (Pose::Cheer, "Khi cậu gõ lệnh mới, phụ tá sẽ cổ vũ cậu nè! Cố lên cậu ơi~! ☆"),
        (Pose::Poke, "Cậu chọc là tớ dỗi đó… (￣^￣) nhưng bấm đúp vào tớ là tớ mở lại terminal cho cậu liền!"),
        (Pose::Held, "Cậu nắm áo len kéo tớ đi đâu cũng được nha! Tớ sẽ đung đưa theo chuột của cậu~ (≧▽≦)"),
        (Pose::Dizzy, "Nhưng đừng lắc tớ mạnh quá… không là tớ chóng mặt lắm đó! (@_@)"),
        (Pose::Music, "Khi yên tĩnh một lúc, tớ sẽ mở nhạc cho cậu nghe~ ♪ Cậu thêm playlist riêng cũng được nha!"),
        (Pose::Excited, "Chán hả? Chọn \"Chơi với tớ\" là mình chơi đoán chữ nè! Tớ háo hức quááá~ (★ω★)"),
        (Pose::Smug, "Nếu phụ tá thắng, cậu sẽ thấy gương mặt này. Hihihi~ (¬‿¬)"),
        (Pose::Pouty, "Còn nếu cậu thắng… hứ. Thôi đượccc. Cậu may mắn thôi! (T_T)"),
        (Pose::Idle, "Cậu bấm chuột phải vào tớ bất cứ lúc nào để mở nhạc, chơi game, đổi hoạt ảnh và kích cỡ nha. Cùng vibecode thật vui nào~! (◕‿◕)✿"),
    ],
    thinking: &[
        "Hmmm… để tớ nghĩ một từ thật khó… (・ω・)",
        "Chờ chút, phụ tá đang chọn từ hay nè~",
        "Hì hì, để tớ tìm từ nào thật khó cho cậu nghẹn xương lun… (¬‿¬)",
        "Ồ, tớ nghĩ ra rồi… à không… okeee, sắp xong đây, đợi tớ xí nha! (^-^)~",
        "Đang vắt óc tìm từ khó cho cậu chơi nè~ ✧",
    ],
    start: &[
        "Tớ có một từ hay lắm! Cậu đoán được không~? (^▽^)",
        "Phụ tá chọn một từ khó lắm đó… chúc cậu may mắn nha! (｀・ω・´)",
        "Đến giờ chơi rồi! Đừng để phụ tá thắng nha~ ☆",
        "Hì hì, sẵn sàng chưa? Từ này hóc búa lắm! (¬‿¬)",
        "Tớ cá là cậu không đoán được ra đâu~! (￣▽￣)",
        "Sẵn sàng… cậu đoán đi nào! Tớ háo hức quááá~! (★ω★)",
        "Lên nàooo~ cho tớ xem tài năng của cậu đi! \\(^o^)/",
        "Suy nghĩ kỹ nha~ phụ tá đang nhìn đó! (・ω・)",
    ],
    hit: &[
        "Hả?! Sao cậu biết chữ đó? (°ロ°)",
        "Ồ, đoán hay lắm~! (o^▽^o)",
        "Hmmm, cậu sắp ra rồi đó… (・_・)",
        "Ăn may ghê! Tiếp đi cậu~ ✧",
        "K-khoan, chữ đó đúng rồi! (⊙_⊙)",
        "Không đượccc, cậu tìm ra một chữ rồi! (>_<)",
        "Okeee, cũng không tệ… không tệ chút nào~ (￣ー￣)",
        "Cậu đang lên tay đó! Làm phụ tá toát mồ hôi luôn~ (；▽；)",
    ],
    miss: &[
        "Sai rồi~! Thử lại nha! (^▽^)",
        "Hihi, không phải chữ đó đâu~ (¬‿¬)",
        "Phụ tá đang thắng nè~! ☆",
        "Sai béttt! Hihihi~ (￣▽￣)",
        "Ồ, gần đúng lắm rồi… mà không phải! (*≧ω≦)",
        "Bíp bíp! Chữ đó không có trong từ đâu~",
        "Ừm ừm, không phải nha! (・ω・)",
        "Tiếc ghê, sai mất rồi~ cố lên cậu! (◕‿◕)",
    ],
    last_heart: &[
        "Á, còn một tim thôi! Cậu nghĩ kỹ nha~ (°ロ°)",
        "Tim cuối rồi… cậu run chưa~? (¬‿¬)",
        "Sai thêm một lần là phụ tá thắng đó! Hihi~ ☆",
        "Ồ, căng thẳng quááá~ (⊙_⊙)",
        "Cẩn thận cẩn thận~ chỉ còn một tim thôi! (>_<)",
    ],
    player_won: &[
        "Xììì~ cậu ăn may thôi! …Nhưng cũng tạm được đó. (￣^￣)",
        "Không đượccc~ cậu đoán ra rồi! Không chịu đâuuu, lần này cậu thắng tớ rồi. (T_T)",
        "Hứ! cậu gặp may thôi… nhưng cũng giỏi ghê! (¬_¬)",
        "Không công bằng~! Cậu chơi chả hay gì cả (Hay lắm ý~) ! (>_<)",
        "Huhu, phụ tá thua rồi… cậu giỏi thật đó! (´・ω・`)",
        "Grrr… chơi lại đi! Mà… cậu cũng không tệ~ (・`ω´・)",
        "Cái gìiii?! Sao vậy được?! …Thôi, chúc mừng cậu nha~ (；▽；)",
        "Cậu thắng… ván này thôi! Phụ tá sẽ páo thù! (｀・ω・´)",
    ],
    player_lost: &[
        "Hihi~ tớ thắng rồi! Từ đó là {w}! (￣▽￣)",
        "Phụ tá chiến thắng~! Là từ {w} đó! \\(^o^)/",
        "Hihihi, khó quá hả cậu? Là từ {w} nè~ (¬‿¬)",
        "Phụ tá lại thắng! Từ ấy là {w}, dễ mà~ ✧",
        "Lần sau cố gắng hơn nha! Đáp án là {w}! (^▽^)",
        "Yeee, tớ thắng~! {w}! Phụ tá vô địch luôn! (★ω★)",
        "Là từ {w} đó! Gần lắm… nhưng chưa đủ gần~ (￣ー￣)",
        "Người thắng là phụ tá~! Từ đó là {w}! ☆",
    ],
    sudoku_start: &[
        "Chơi Sudoku nè~ Chọn một ô rồi chọn số nha! (｀・ω・´)",
        "Tớ làm riêng một câu đố cho cậu nè~ ✧",
        "Điền 1 đến 9 vào mỗi hàng, cột và khối nha~ cố lên! ☆",
    ],
    sudoku_won: &[
        "Yeee, cậu giải xong rồi! Giỏi quá đi~! (★ω★)",
        "Số nào cũng đúng chỗ hết~ đỉnh thật! \\(^o^)/",
        "Xong câu đố! Tớ phục cậu luôn đó~ ✧",
    ],
    commands: [
        &["Đang chạy test… cầu mong pass hết ha~! (>_<)", "Giờ test nè! Qua điii, qua điii…", "Kiểm tra xem có gì hỏng không~ (・ω・)", "Test, test~ một, hai, ba! ☆", "Cố lên mấy dấu tích xanh~! (｀・ω・´)"],
        &["Đang mò git nè~ (・ω・)", "Lật lại sổ lịch sử! ✧", "Gõ lệnh git hả? Để phụ tá lo~ (￣▽￣)", "Lưu tiến độ nè~ ☆", "Xem có gì thay đổi nào~ (・_・)"],
        &["Đi mua sắm package nè~! (^▽^)", "Lấy thêm đồ chơi… ✧", "Đang cài đặt, chờ tớ chút nhaaa~", "Đồ chơi mới sắp tới~! (★ω★)", "Đang tải, đang tải… (・ω・)"],
        &["Xây từng chút một nè~ (｀・ω・´)", "Code đang compile… đoạn chán nhất luôn ý! (￣ー￣)", "Gõ gõ ráp ráp lại hết nè~ ☆", "Build build build~! ♪", "Ghép các mảnh lại với nhau… (・_・)"],
        &["Cho chạy thử một vòng nè~! (^▽^)", "Xem nó có chạy không nào! (⊙_⊙)", "Khởi động… đi điii~! ☆", "Cầu mong nó chạy được~ (>_<)", "Brừm brừm~ chạy thôi! \\(^o^)/"],
        &["Cốc cốc, internet ơi~! (・ω・)", "Đang lấy đồ trên mạng nè! ✧", "Hỏi internet thật lịch sự~ (◕‿◕)", "Tải một chút gì đó về~ ♪"],
        &["Dọn dẹp mấy file nè… (・_・)", "Vứt đồ cũ đi~ một cách thật cẩn thận, cẩn thận, cẩn thận!!! (°ロ°)", "Giờ dọn dẹp~ ✿", "Tạm biệt file cũ nha~ (￣▽￣)"],
        &["Ngó nghiêng mấy file nè~ (・ω・)", "Lục lọi các thư mục! (¬‿¬)", "Chỉ xem qua một chút thôi~ ✧", "Đang tìm, đang tìm, đang tìm hông ra… (⊙_⊙)", "Ồ, trong này có gì nhỉ~? (◕‿◕)"],
        &["Đang chạy lệnh… (・ω・)", "Gõ pháp thuật vào terminal nè~ ✧", "Lệnh ơi, chạy điii~! ☆", "Bíp bíp, đang nói chuyện với bạn terminal~", "Dễ ẹt, phú tá làm trong phút mốt! (｀・ω・´)"],
    ],
};

pub fn lines() -> &'static Lines {
    match lang() {
        Lang::En => &EN_LINES,
        Lang::Vi => &VI_LINES,
    }
}

/// Every line recorded ahead of time, with its language code: needing your OK (the main one), working fillers,
/// idle nudges, pokes, music, goodbyes, game reactions and the tour. Not shell lines (too many, said too often), and
/// not lines with the game word in them (unknown ahead of time).
pub fn voiced() -> Vec<(&'static str, &'static str)> {
    let mut out = vec![];
    for (l, code) in [(&EN_LINES, "en"), (&VI_LINES, "vi")] {
        for set in [l.waiting, l.working, l.idle, l.poke, l.dizzy, l.coffee, l.music, l.cheer, l.bye, l.thinking, l.start, l.hit, l.miss, l.last_heart, l.player_won, l.player_lost, l.sudoku_start, l.sudoku_won] {
            out.extend(set.iter().filter(|s| !s.contains('{')).map(|s| (*s, code)));
        }
        out.extend(l.tour.iter().map(|(_, s)| (*s, code)));
    }
    out
}

/// A playlist's name for display: the built-in one and loose files get translated names.
pub fn playlist_name(name: &str) -> &str {
    match name {
        crate::music::BUILT_IN => ui().built_in,
        crate::music::LOOSE => ui().my_music,
        _ => name,
    }
}

pub fn mode_label(mode: crate::music::Mode) -> &'static str {
    match mode {
        crate::music::Mode::Loop => ui().mode_loop,
        crate::music::Mode::LoopOne => ui().mode_loop_one,
        crate::music::Mode::Shuffle => ui().mode_shuffle,
    }
}

/// Her answer when asked for a slash command a hosted session doesn't offer.
pub fn cant_slash(lang: Lang, command: &str) -> String {
    match lang {
        Lang::En => format!("Ehh, your sidekick can't do /{command} from here, sorry~ (・ω・)"),
        Lang::Vi => format!("Ơ, tớ không làm được /{command} từ đây rồi, xin lỗi cậu nha~ (・ω・)"),
    }
}

/// Vietnamese letters without their accents and tone marks (đ becomes d), for comparing words typed on an A-Z grid.
pub fn fold(text: &str) -> String {
    const GROUPS: [(&str, char); 12] = [
        ("àáạảãâầấậẩẫăằắặẳẵ", 'a'), ("ÀÁẠẢÃÂẦẤẬẨẪĂẰẮẶẲẴ", 'A'), ("èéẹẻẽêềếệểễ", 'e'), ("ÈÉẸẺẼÊỀẾỆỂỄ", 'E'),
        ("ìíịỉĩ", 'i'), ("ÌÍỊỈĨ", 'I'), ("òóọỏõôồốộổỗơờớợởỡ", 'o'), ("ÒÓỌỎÕÔỒỐỘỔỖƠỜỚỢỞỠ", 'O'),
        ("ùúụủũưừứựửữ", 'u'), ("ÙÚỤỦŨƯỪỨỰỬỮ", 'U'), ("ỳýỵỷỹ", 'y'), ("ỲÝỴỶỸ", 'Y'),
    ];
    text.chars()
        .map(|c| match c {
            'đ' => 'd',
            'Đ' => 'D',
            _ => GROUPS.iter().find(|(set, _)| set.contains(c)).map_or(c, |(_, base)| *base),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_every_line_set() {
        for l in [&EN_LINES, &VI_LINES] {
            for set in [l.idle, l.bye, l.working, l.poke, l.dizzy, l.coffee, l.waiting, l.music, l.cheer, l.thinking, l.start, l.hit, l.miss, l.last_heart, l.player_won, l.player_lost, l.sudoku_start, l.sudoku_won] {
                assert!(set.len() >= 3);
            }
            assert!(l.commands.iter().all(|c| c.len() >= 3));
            assert!(l.player_lost.iter().all(|s| s.contains("{w}")));
        }
        assert_eq!(EN_LINES.tour.len(), VI_LINES.tour.len());
        assert!(EN_LINES.tour.iter().zip(VI_LINES.tour).all(|(a, b)| a.0 == b.0)); // same poses, same order
    }

    #[test]
    fn folds_vietnamese_marks() {
        assert_eq!(fold("Mèo con đi học, bánh mì Đà Nẵng"), "Meo con di hoc, banh mi Da Nang");
        assert_eq!(fold("Trăng Sáng"), "Trang Sang");
    }
}
