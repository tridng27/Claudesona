# Claudesona art prompts

Eighteen images (one blink is optional), made with any AI image tool that accepts a reference image.

## Steps

1. Make the master image with prompt **1 (idle)**. Regenerate until you love her: every other image copies this one.
2. For every other pose, attach the master image as a reference and paste that pose's prompt.
3. Blink images: don't generate them from scratch. Use your tool's edit/inpaint on the open-eyes image and change only the eyes. If anything else changes, she'll twitch every time she blinks.
4. Background: choose "transparent" if your tool offers it. Otherwise keep the flat green background and remove it afterwards (remove.bg, Photoshop, or the free `rembg` tool).
5. Save as PNG in this folder with the exact file names below. Keep every image the same canvas size, with her at about the same size. The app lines her up by her feet.

## Blocks to paste

Replace `[Character]` and `[Style]` in each prompt with these, word for word. Identical wording helps keep her consistent.

**[Character]**
> Claudesona, a chibi anime girl about 2.5 heads tall, cheerful and full of energy. Shoulder-length wavy hair in warm coral-terracotta (#D97757) with a small cream eight-pointed sparkle hair clip on her left side. Big round amber eyes with tiny star highlights, rosy cheeks. Oversized cream knit sweater (#F5EBDD) with coral trim on the collar and cuffs, sleeves covering half her hands. Short coral pleated skirt, cream knee socks, brown loafers.

**[Style]**
> Cute mascot sticker style: clean bold outlines, soft cel shading, flat warm colors. Full body, front view, centered, whole figure visible with empty space around her. Transparent background, or a plain flat #00FF00 green background if transparency isn't available. No ground shadow, no text, no symbols, no other objects unless described.

## Prompts

### 1. `idle.png` (master)
> [Character] She stands relaxed with her hands clasped in front of her, head tilted slightly, eyes open, gentle happy smile. [Style]

### 2. `idle_blink.png` (edit of idle.png)
> Edit this image: close her eyes into happy curved lines, like ^ ^. Change nothing else.

### 3. `working.png`
> [Character] Same character as the reference image. She stands typing on a small glowing coral holographic keyboard floating in front of her waist, thinking very hard: eyes open but narrowed in concentration, glancing slightly upward, brows furrowed together. Her mouth is a small closed pointed shape like an upside-down V, as in "^". [Style]

### 4. `working_blink.png` (edit of working.png)
> Edit this image: close her eyes, keeping the furrowed brows and the "^" mouth. Change nothing else.

### 5. `waiting.png`
> [Character] Same character as the reference image. She stands proud of herself: hands on her hips, chest puffed out, chin raised a little, big confident grin. Her eyes are sparkling starry eyes: bright star-shaped highlights filling her pupils. [Style]

### 6. `poke.png`
> [Character] Same character as the reference image. She has just been poked and is a bit annoyed: half-lidded unimpressed eyes looking toward the viewer, one cheek puffed out in a small pout, brows slightly lowered, arms crossed, a light blush. Mildly grumpy, not angry. [Style]

### 7. `music.png` (no blink image needed: her eyes are already closed)
> [Character] Same character as the reference image. She is quietly enjoying music, serene and peaceful: eyes gently closed into soft relaxed curves, a small calm smile, a faint blush, head tilted slightly to her left. Her right hand (on the viewer's left) is raised out to the side at about shoulder height, well away from her body and hair, palm facing down and fingers loose and relaxed, as if softly swaying along to the beat. Her left hand rests lightly at her side. No headphones, no accessories, nothing added beyond the reference outfit. Same standing position and body size as the reference, feet together on the same spot. [Style]

Tips for this one:
- The app sways her raised hand to the beat, so the hand and sleeve need empty space around them: no overlap with her hair, face or body. If the tool tucks the hand in close, regenerate.
- If the tool adds music notes or headphones anyway, regenerate or erase them: the app draws the floating notes itself.
- Check that her hair outline and sweater still match the master.

### 8. `game_excited.png` (shown the whole time you play "Play with me")
> [Character] Same character as the reference image. She is super excited to play a guessing game with the viewer: big sparkling wide-open eyes, a huge open-mouthed grin, both fists clenched happily up near her chin, shoulders raised a little with anticipation, leaning slightly toward the viewer, rosy cheeks. Same standing position and body size as the reference, feet together on the same spot. [Style]

### 9. `game_excited_blink.png` (optional, edit of game_excited.png)
> Edit this image: close her eyes into happy curved lines, like ^ ^, keeping the big grin. Change nothing else.

### 10. `game_smug.png` (she won the game)
> [Character] Same character as the reference image. She just won a game against the viewer and is playfully smug: half-closed eyes looking at the viewer, one eyebrow slightly raised, a sly closed-mouth smirk, one hand raised with the back of her fingers near her mouth as if giggling "ohoho", the other hand on her hip, chin tilted up a little. Teasing and cute, not mean. Same standing position and body size as the reference, feet together on the same spot. [Style]

### 11. `game_pouty.png` (you won the game)
> [Character] Same character as the reference image. She just lost a game to the viewer and is a bit sad but a good sport: eyes slightly teary and glancing to the side, small pout with her bottom lip pushed out, cheeks lightly puffed and blushing, brows tilted up in the middle, hands clasped behind her back, shoulders a little slumped. Disappointed but cute, not crying. Same standing position and body size as the reference, feet together on the same spot. [Style]

If one of these is missing (e.g. in a custom skin), the app borrows another pose: idle for excited, waiting for smug, poke for pouty, waiting for cheer, excited for held, idle for coffee.

### 12. `cheer.png` (shown while you type into a Claude session)
> [Character] Same character as the reference image. She is cheering the viewer on while they work, like an enthusiastic little supporter: her right arm (on the viewer's left) thrust straight up high above her head in a big "you can do it!" pose, hand in a small happy fist, sleeve slipping down a little toward her elbow. The raised arm and fist are well away from her hair and face, with clear empty space around them. Her left hand rests on her hip. Bright wide-open sparkling eyes looking at the viewer, a big open-mouthed encouraging smile, rosy cheeks, shoulders lifted with energy. No pom-poms, no props, nothing added beyond the reference outfit. Same standing position and body size as the reference, feet together on the same spot. [Style]

### 13. `cheer_blink.png` (edit of cheer.png)
> Edit this image: close her eyes into happy curved lines, like ^ ^, keeping the big smile and raised arm. Change nothing else.

Tips for this one:
- Her raised arm must not overlap her hair. The app bends each pose on a grid, so overlapping parts would warp together. If the tool tucks the arm in close, regenerate.
- If the tool raises both arms anyway, regenerate.
- Make sure the fist stays inside the canvas with some margin above it. A raised arm is easy to crop.
- Don't let her feet rise off the ground or move: the app lines her up by her feet.

### 14. `held.png` (shown while you drag her)
> [Character] Same character as the reference image. She is being picked up by the back of her sweater collar like a kitten and is dangling in the air, thrilled about it: big sparkling wide-open eyes, a huge open-mouthed delighted grin, rosy cheeks. The back of her sweater collar is pulled up high, as if held from above (don't draw the hand), so the back of her neck is the highest point of her body. Her upper body bends forward and her head droops down in front of the collar, chin tucked toward her chest, but her face still turns up toward the viewer with that delighted grin. Her arms hang loosely out to her sides, slightly away from her body, sleeves drooping. Her right leg (on the viewer's left) dangles straight down, relaxed, toes pointing a little down. Her left leg (on the viewer's right) is playfully bent up at the knee, kicked back so the lower leg points behind her and the loafer sole shows, like a happy little kick. A clear gap between the two legs from the knees down, loafers still on. Same body size as the reference, front view. [Style]

### 15. `held_blink.png` (edit of held.png)
> Edit this image: close her eyes into happy curved lines, like ^ ^, keeping the big grin. Change nothing else.

Tips for this one:
- The app swings her from the collar and moves each leg on its own, so the legs must not touch or cross below the skirt. If they do, regenerate.
- If the tool bends both legs or neither, regenerate. The bent foot must stay inside the canvas.
- Her hands must not overlap her legs or skirt hem.
- The grabbed collar should sit roughly where her neck is in the master image, centered left to right. The app hangs her from that spot.
- Her face must stay visible and happy, not hidden by the bend.

### 16. `held_dizzy.png` (edit of held.png, shown when she's shaken too much)
> Edit this image: turn her eyes into black spirals, like @ @, and her mouth into a small closed wobbly line. Change nothing else.

### 17. `coffee.png` (shown when you give her coffee from the menu)
> [Character] Same character as the reference image. She is happily sipping from a warm cream-coloured coffee mug with a small coral heart on it, holding it with both hands in front of her chin, sleeves covering half her hands. Eyes gently closed into content happy curves, rosy cheeks, a tiny blissful smile at the rim of the mug, no steam (the app draws it). Same standing position and body size as the reference, feet together on the same spot. [Style]

Tips for this one:
- Keep the mug and hands in front of her chin, clear of her side hair: the app sways the hair tips.
- No hearts or steam: the app draws those (and animates the steam).

### 18. `coffee_blink.png` (edit of coffee.png)
> Edit this image: close her eyes into soft relaxed downward curves, like ‿ ‿. Change nothing else.


- Mini versions: the app reuses these images at half size.
- Effects like the "!" above her head, sparkles, sweat drops and music notes: the app draws them.
