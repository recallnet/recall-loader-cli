# recall-loader

A simple cli for perf/load testing with the recall cli (buckets and blobs)

## Storage

If you're running the devnet, you'll probably want to clean out your iroh folder peridically as you'll be storing a lot of junk in there. Or even run iron with a local folder instead of the default ( on osx `~/Library/Application\ Support/iroh`).

## Usage

By default, blobs are uploaded to `/foo/$x` where x is the number. You can change the bucket prefix with the `--prefix` argument.

```sh
cargo build --release

❯ ./target/release/recall-loader -h
Usage: recall-loader <COMMAND>

Commands:
  basic-test  Run a basic test using cli args
  cleanup     Clean up (delete) data from a bucket
  run-test    Run a more sophisticated test from a config file
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version

❯ ./target/release/recall-loader basic -h
Run a basic test using cli args

Usage: recall-loader basic-test [OPTIONS] --key <KEY>

Options:
  -p, --prefix <PREFIX>              Everything under /foo by default in bucket can use `date +"%s"` to get the unix epoch seconds for a 'random' value for the test [default: foo]
  -k, --key <KEY>                    The private key to use for the signer wallet [env: RECALL_PRIVATE_KEY=]
  -n, --network <NETWORK>            The network to use (defaults to devnet) [env: RECALL_NETWORK=devnet]
  -b, --bucket <BUCKET>              The bucket machine address (fvm address string)
      --buy-credits <BUY_CREDITS>    The count of credits to buy before starting (defaults to not buying any)
      --delete                       whether blobs should be deleted afterward
      --download                     whether to query and download blobs after uploading them
  -c, --blob-cnt <BLOB_CNT>          [default: 100]
  -s, --blob-size-mb <BLOB_SIZE_MB>  blob size in mb (0.1 = 100 bytes, 1000 = 1gb) [default: 1.0]
  -h, --help                         Print help

```

Uses the `RECALL_NETWORK` and `RECALL_PRIVATE_KEY` variables if nothing passed to the cli. Will create a new bucket if none is specified.

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
# create a new bucket and upload 4 100 byte blobs (requires RECALL_PRIVATE_KEY to be set, defaults to devnet)
./target/release/recall-loader basic -c 4 -s .1
# upload 3 1gb blobs to testnet
./target/release/recall-loader basic --bucket $IGNITION_BUCKET -n testnet -k $IGNITION_PRIVATE_KEY -s 1000 --blob_cnt 3
# upload, query and delete the blobs after uploading (defaults to 100 1mb blobs in a new bucket with /foo prefix)
./target/release/recall-loader basic  --delete --query
# delete blobs from a bucket. if it fails to list the bucket due to out of gas, will not delete anything
./target/release/recall-loader cleanup --bucket $IGNITION_BUCKET -n testnet -k $IGNITION_PRIVATE_KEY --prefix foo/
# run a test using a config file where you can specify multiple tests in parallel
 ./target/release/recall-loader run -p ./test-config/upload.json 
```

```jsonc
{
    "funderPrivateKey" : "FUNDER_PRIVATE_KEY",
    "network": "devnet",
    "test": {
        "numAccounts": 1,
        "requestFunds" : 6,
        "buyCredit": 5,
        "target": "sdk",
        "upload": {
            "bucket": null,
            "blobCount": 10,
            "prefix": "bar",
            "blobSize": 102400,
            "overwrite": true
        },
        "download": true,
        "delete": false
    }
}
```
