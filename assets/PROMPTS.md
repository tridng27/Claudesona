# Claudesona art prompts

Ten images (plus one optional blink), made with any AI image tool that accepts a reference image.

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

If one of these is missing (e.g. in a custom skin), the app borrows another pose: idle for excited, waiting for smug, poke for pouty.

## Not needed

- Mini versions: the app reuses these images at half size.
- Effects like the "!" above her head, sparkles, sweat drops and music notes: the app draws them.
