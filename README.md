# hoku-loader

A simple cli for perf/load testing with the hoku cli (buckets and blobs)

## Storage

If you're running the devnet, you'll probably want to clean out your iroh folder peridically as you'll be storing a lot of junk in there. Or even run iron with a local folder instead of the default ( on osx `~/Library/Application\ Support/iroh`).

## Usage

By default, blobs are uploaded to `/foo/$x` where x is the number. You can change the bucket prefix with the `--prefix` argument.

```sh
cargo build --release

❯ ./target/release/hoku-loader -h
Usage: hoku-loader <COMMAND>

Commands:
  basic-test  Run a basic test using cli args
  cleanup     Clean up (delete) data from a bucket
  run-test    Run a more sophisticated test from a config file
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

❯ ./target/release/hoku-loader basic -h
Run a basic test using cli args

Usage: hoku-loader basic-test [OPTIONS] --key <KEY>

Options:
  -p, --prefix <PREFIX>              Everything under /foo by default in bucket can use `date +"%s"` to get the unix epoch seconds for a 'random' value for the test [default: foo]
  -k, --key <KEY>                    The private key to use for the signer wallet [env: HOKU_PRIVATE_KEY=]
  -n, --network <NETWORK>            The network to use (defaults to devnet) [env: HOKU_NETWORK=devnet]
  -b, --bucket <BUCKET>              The bucket machine address (fvm address string)
      --buy-credits <BUY_CREDITS>    The count of credits to buy before starting (defaults to not buying any)
      --delete                       whether blobs should be deleted afterward
      --download                     whether to query and download blobs after uploading them
  -c, --blob-cnt <BLOB_CNT>          [default: 100]
  -s, --blob-size-mb <BLOB_SIZE_MB>  blob size in mb (0.1 = 100 bytes, 1000 = 1gb) [default: 1.0]
  -h, --help                         Print help

```

Uses the `HOKU_NETWORK` and `HOKU_PRIVATE_KEY` variables if nothing passed to the cli. Will create a new bucket if none is specified.

### Examples

You can add more addresses to devnet to deploy using by adding something like this to `scripts/deploy.sh`

```sh
for NAME in ellie fonzi grape; do
  fendermint key gen --out-dir test-network/keys --name $NAME;
done

# Add accounts to the Genesis file
## A stand-alone account
fendermint genesis --genesis-file test-network/genesis.json add-account --public-key test-network/keys/alice.pk --balance 1000 --kind ethereum
fendermint genesis --genesis-file test-network/genesis.json add-account --public-key test-network/keys/ellie.pk --balance 1000 --kind ethereum
fendermint genesis --genesis-file test-network/genesis.json add-account --public-key test-network/keys/fonzi.pk --balance 1000 --kind ethereum
```

```sh
# create a new bucket and upload 4 100 byte blobs (requires HOKU_PRIVATE_KEY to be set, defaults to devnet)
./target/release/hoku-loader basic -c 4 -s .1
# upload 3 1gb blobs to testnet
./target/release/hoku-loader basic --bucket $IGNITION_BUCKET -n testnet -k $IGNITION_PRIVATE_KEY -s 1000 --blob_cnt 3
# upload, query and delete the blobs after uploading (defaults to 100 1mb blobs in a new bucket with /foo prefix)
./target/release/hoku-loader basic  --delete --query
# delete blobs from a bucket. if it fails to list the bucket due to out of gas, will not delete anything
./target/release/hoku-loader cleanup --bucket $IGNITION_BUCKET -n testnet -k $IGNITION_PRIVATE_KEY --prefix foo/
# run a test using a config file where you can specify multiple tests in parallel
 ./target/release/hoku-loader run -p ./test-config/upload.json 
```

```jsonc
{
    "privateKey": "USED_FOR_ALL_TESTS_BY_DEFAULT",
    "network": "devnet",
    "tests": [
        {
            // specify a key to use for this test
            "privateKey": null,
            "buyCredit": 10,
            "test": {
                "upload": {
                    // create a new bucket by default
                    "bucket": null,
                    "blobCount": 10,
                    "prefix": "bar",
                    // 1 MB blobs
                    "blobSizeMb": 1.0,
                    // whether to overwrite existing blobs in the bucket
                    "overwrite": true
                },
                // Download the complete blob after uploading (or null/false to skip)
                "download": true,
                // Delete the blobs before existing the test
                "delete": true
            }
        },
        {
            "privateKey": "OVERRIDES_THE_ROOT_PRIVATE_KEY",
            "buyCredit": 10,
            "test": {
                "upload": {
                    "bucket": null,
                    "blobCount": 2000,
                    "prefix": "foo",
                    // 10 MB blobs
                    "blobSizeMb": 10.0,
                    "overwrite": true
                },
                // commit, async, sync (commit by default)
                "broadcastMode": "sync",
                // Download a range of the blobs after uploading (waiting for existence)
                "download": "0-99",
                // Delete the blobs before existing the test
                "delete": true
            }
        }
    ]
}
```
