# Match a hall reverb with an impulse response

Export the Python hall's impulse response as a mono or stereo WAV at the Crump Project rate. Include the 14 early reflections and the frequency-dependent decay in the file. Export at the original linear gain and start the WAV at the hall's first sample. Apply the 22 ms pre-delay in Crump if the WAV leaves it out.

Copy the file into the Project as `media/game-hall.wav`. Replace the example digest with the lowercase SHA-256 of the complete WAV. Add the declaration at Project scope in a `crump 2` source.

```crump
asset game_hall "Game hall" wav "media/game-hall.wav" sha256 "<sha256>" {
  usage impulse_response
}
```

Add the Return to the Song. Its input EQ high-passes the feed at 200 Hz. Use the same feed filter and cutoff as the Python mixer when matching its response.

```crump
return hall "Game hall" level 0db {
  processor hall_feed eq {
    setting high_pass 200hz
  }
  processor hall_ir convolution {
    setting ir game_hall
    setting mix 1
    setting pre_delay 22ms
  }
  output master
}
```

Add `send to_hall target return/hall tap post level -12db` inside any Track that should feed the hall. Adjust the send to match the Python mix. If the WAV already includes the feed filter or pre-delay, omit the corresponding processing here. Set the Project's `render_tail` or the bounce tail to cover the WAV duration plus any separate pre-delay and upstream filter decay.
