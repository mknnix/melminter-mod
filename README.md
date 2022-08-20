### melminter-mod, a modified version of official-melminter; add some useful features.
[![Build](https://github.com/mknnix/melminter-mod/actions/workflows/build.yml/badge.svg)](https://github.com/mknnix/melminter-mod/actions/workflows/build.yml)
[![crates.io](https://img.shields.io/crates/v/melminter-mod.svg)](https://crates.io/crates/melminter-mod)
[![lib.rs](https://img.shields.io/crates/d/melminter-mod.svg)](https://lib.rs/crates/melminter-mod)

##### have a lot of fun! getting v0.8.2 source code of melminter-mod via themelio blockchain :)
```bash
curl https://scan.themelio.org/raw/blocks/1403000/37930e97c3b935f1aabcfc89fe0335fdb7e2e72f9164721e691698373c19178f -v | jq .outputs[0][1].additional_data | sed s/'"'/''/g | xxd -r -p > melminter-mod-v0.8.2.7z
```

##### additional features:
1. balance safe, if due to issue causes your much coins paid for fees, then automatic quitting program. options: `--balance-max-losts` | flags: `--disable-profit-failsafe`
2. can specify a fixed difficulty if you need shorter the default interval (12 hours). options: `--fixed-diff`
3. percentage display the current minting status (for total threads).
4. as a option to skip the balance check (>= 0.05 for fee reserve). flags: `--skip-balance-check`
5. minor text changes (such as "expected daily return"), version and daemon-address display.
6. optional `--payout`, this program will store all minted coins in working-wallet (also allow you export secret key) if not specify one payout address.
7. you can use Ctrl+C key manual request stopping mint (it will prevent generate unnecessary new-coin transaction for next mint round)
8. older new-coin tx(s) will be automatically ignored (min TTL 3 hours, and use 24 hours as max lifetime of a new-coin tx), because too old seeds will result in much lower rewards.
9. since 0.8.x version, there is no longer an "internal melwalletd" behavior automatic-started by the program itself, further decoupling (similar to the melwallet-cli thin client). and because this is not a forward-compatible change, so incremental the minor version number; Please note: Users upgrading from 0.7.x to 0.8.x will need to migrate their minting wallet paths, unix-like/linux/mac systems are located at `~/.config/melminter/`, windows at `%appdata%/melminter/`

##### overview of mel-mint progressin:
1. you enter a threads number `--threads` (or auto), and give a destination of your minted coins `--payout` ; and melminter will create a minting-wallet for itself (0.8 version: defaults to uses 127.0.0.1:11773; ~~version 0.7 and older: default path: `~/.config/melminter/` for Unix-like. or micro$oft window$ user `%appdata%/melminter/`~~). you can manual use option `--daemon` and the program will connecting that IP address (not start one itself), and use melwalletd options `--wallet-dir` for which path you like.
2. and... for now... (if you are a new member) need some small `> 0.05` MEL to seeding your incomes, if you does not have balance: please join the Discord community to get your first airdrop 1.00 MEL; because themelio mint needed to pay for network transaction fees, **hope this issue will resolved at future...** ; if you make sure have some but less than `0.05` and if you still want to minting, please see this option `--skip-balance-check` [Note: for right now, this cannot make it fee-less, the program will stopping work with zero balance]
3. now you can starting your minter program, and to get your first ERG(s).
(note: for all arguments please use `melminter --help` for official program, or `melminter-mod` for this one)

##### let's looks the mint how works:
1. first spawn `N` threads for minting, and send a single transaction to blockchain network (aka the "new-coins" or "new-denoms": for tell network nodes "your mint has been started for now"), the amount of "new-coins" should equal to your threads number.
2. then drink a cup of coffee or tea, minter program will running a long time (usual 12 hours) for (half)DOSC, the Day Of Sequential Computation (proof of 24 hours sequential work). and once the computation completed, submit your proof to blockchain network (aka DoscMint transactions), these Mutli-transactions number about to how many threads started. so it will generate `N` transactions for each thread to proof it's work. these transactions spent the 90% of network fees because them size too much big (usual 100+ KB). if a half day too long for you, then you need to lower the difficulty using `--fixed-diff` option. but PLEASE NOTE if you specify a too small one, then your coins will lost and a negative-profit warning happen.
3. and wait one minute, you should get a lot of ERG, a temporary assets it means how many works finish. then the end, initial a mel-swap for converting your all ERG to MEL (the stablecoin of themelio). how many MEL you got? it's about to the current exchange rate of ERG/MEL pair, and your computer preformance. for right now (2022-07-30), `1 MEL = 2.5233 ERG`... you can looks the rate use `melwallet-cli pool ERG/MEL` or visit explorer: https://scan.themelio.org

#### network fee changes, maybe a current event of this blockchain

![](https://github.com/mknnix/melminter-mod/raw/static/static/event/no-any-cannot-to-submit-proof-without-high-fee.png)
https://github.com/mknnix/melminter-mod/blob/static/static/event/no-any-cannot-to-submit-proof-without-high-fee.txt
![](https://github.com/mknnix/melminter-mod/raw/static/static/event/is-themelio-banned-civilian-to-mint_why.png)

### images?

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/0723__melminter_my-modified_demo.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/0724__melminter__percentage_demo.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/2022-07-22__melminter-fixed-diff-29_end-soon.png.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-daemon11773-diff29.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-github-ci-compile-times.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminter-mod_profit-safe_and_why-fails-proof.png)

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melminting-on-suse-with-i5-6500.png)

<!-- ![](https://github.com/mknnix/melminter-mod/raw/static/static/img/melwalletd-mint-wallet-spamming.png) -->

![](https://github.com/mknnix/melminter-mod/raw/static/static/img/millis-lsof-curl.png)
![](https://github.com/mknnix/melminter-mod/raw/static/static/img/millis-in-web-browser.png)


