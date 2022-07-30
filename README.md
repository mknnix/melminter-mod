a modified version of official-melminter; add some usual features.

overview of mel-mint starters:
1. you enter a threads number `--threads` (or auto), and give a destination of your mints `--payout` ; and melminter will create a minting-wallet for itself (default path: `~/.config/melminter/` for Unix-like. ~~or micro$oft window$ user `%appdata%/melminter/`~~), but you can manual use option `--daemon` and the program will connecting that IP address (not start one itself), and use melwalletd options `--wallet-dir` for which path you like.
2. and... for now... (if you are a new member) need some small `> 0.05` MEL to seeding your incomes, if you does not have balance: please join the Discord community to get your first airdrop 1.00 MEL; because themelio mint needed to pay for network transaction fees, **hope this issue will resolved at future...** ; if you make sure have some but less than `0.05` and if you still want to minting, please see this option `--skip-amount-check` [Note: for now this cannot make it fee-less, the program will stopping work with zero balance]
3. now you can starting your minter program, and to get your first ERG(s).
(note: for all arguments please use `melminter --help` for official program, or `melminter-mod` for this one)

then to looks the mint how works:
1. first spawn `N` threads for minting, and send a single transaction to blockchain network (aka the "new-coins" or "new-denoms": for tell network nodes "your mint has been started for now"), the amount of "new-coins" should equal to your threads number.
2. then drink a cup of coffee or tea, minter program will running a long time (usual 12 hours) for DOSC, the proof of seq work (PoSW). and once the computing completed, submit your proof to the melnet (aka DoscMint transactions), these Mutli-transactions number about to how many threads started. so it will generate `N` transactions for each thread to proof it's work. these transactions spent the 90% of network fees because them too much big size. if the 12 hours too long for you, then you need to lower the difficulty using `--fixed-diff` option. but notice if you specify a too small one, then your coins will lost as result.
3. and wait one minute, you should get a lot of ERG, a temporary assets it means how many works finish. then the end, initial a mel-swap for converting your all ERG to MEL (the stablecoin of themelio). how many MEL you got? it's about to the current exchange rate of ERG/MEL pair. for now (2022-07-30), `1 MEL = 2.5233 ERG`... you can looks the rate use `melwallet-cli pool ERG/MEL` or visit explorer: https://scan.themelio.org

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/0723__melminter_my-modified_demo.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/0724__melminter__percentage_demo.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/2022-07-22__melminter-fixed-diff-29_end-soon.png.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-daemon11773-diff29.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-github-ci-compile-times.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-mod_profit-safe_and_why-fails-proof.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminting-on-suse-with-i5-6500.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melwalletd-mint-wallet-spamming.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/millis-in-web-browser.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/millis-lsof-curl.png)

