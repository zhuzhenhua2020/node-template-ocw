# 第四课
以ocw-exaple为基础，把它拷到assignment目录里来修改最后提交这个代码库.

利用offchain worker取出Dot当前对USD的价格，并写到一个Vec的存储里，您们自己选 一种方法提交回链上，并且在代码注释为什么用这种方法提交回链上最好。

只保留当前最近的10个价格，其他价格可以丢弃（就是Vec的长充到10后，这里再插入一个值时，要先丢弃最早的那个值）。

这个Http请求可以得到当前Dot价格：

https://api.coincap.io/v2/assets/polkadot

## 设置Http请求网址 
```
const HTTP_REMOTE_REQUEST_DOT_PRICE: &str = "https://api.coincap.io/v2/assets/polkadot";
```
## Dot价格结构体
```
    #[derive(Deserialize, Encode, Decode, Default)]
    struct DotPriceInfo {
        #[serde(deserialize_with = "de_string_to_bytes")]
        name: Vec<u8>,
        #[serde(deserialize_with = "de_string_to_bytes")]
        usd: Vec<u8>,
    }

    impl fmt::Debug for DotPriceInfo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{{ name: {}, usd: {} }}",
                str::from_utf8(&self.name).map_err(|_| fmt::Error)?,
                str::from_utf8(&self.usd).map_err(|_| fmt::Error)?
            )
        }
    }
```
## 设置Dot价格存储
```
    #[pallet::storage]
    #[pallet::getter(fn prices)]
    pub type Prices<T> = StorageValue<_, VecDeque<(u64, Permill)>, ValueQuery>;
```
## 获取DOT当前对USD的价格方法
```
     fn fetch_price_info() -> Result<(), Error<T>> {
            log::info!("...... fetch_price_info");

            let dot_price =
                Self::fetch_n_parse_to_dot_price().map_err(|_| <Error<T>>::GetPriceErr)?;

            // 获取 dot usd 对应的字符串
            let usd_str: &str = str::from_utf8(&dot_price.usd).unwrap();

            //折分成整部分和小数部分的元组
            let price_tuple: (u64, Permill) =
                Self::convert_price_str(usd_str).map_err(|_| <Error<T>>::ConvertPriceErr)?;
            log::info!("...... price_tuple: {:?}", price_tuple);

            // 使用不签名但具签名信息的交易，提交到链上。
            // 因为很多时候签名交易意味签名者需要为该交易付手续费。但有些情况只想知道该交易来源是谁，但不需要该用户付手续费。
            let signer = Signer::<T, T::AuthorityId>::any_account();
            let result = signer.send_unsigned_transaction(
                |acct| PayloadPrice {
                    price_tuple: price_tuple,
                    public: acct.public.clone(),
                },
                Call::submit_price_unsigned_with_signed_payload,
            );
            if let Some((_, res)) = result {
                return res.map_err(|_| {
                    log::error!("...... Failed in offchain_unsigned_tx_signed_payload");
                    <Error<T>>::OffchainUnsignedTxSignedPayloadError
                });
            }
            log::error!("...... No local account available");
            Err(<Error<T>>::NoLocalAcctForSigning)
        }
```
## 获取HTTP数据具体实现
```
        fn fetch_n_parse_to_dot_price() -> Result<DotPriceInfo, Error<T>> {
            // 获取HTTP数据
            let resp_bytes =
                Self::fetch_from_remote(HTTP_REMOTE_REQUEST_DOT_PRICE).map_err(|e| {
                    log::error!("...... fetch_from_remote error: {:?}", e);
                    <Error<T>>::HttpFetchingError
                })?;

            // HTTP 数据，转换成 Json 字符串
            let resp_str =
                str::from_utf8(&resp_bytes).map_err(|_| <Error<T>>::HttpFetchingError)?;
            log::info!("{}", resp_str);

            // 方法2： 转换为serde_json 再转换成 dot_price
            let v: serde_json::Value =
                serde_json::from_str(&resp_str).map_err(|_| <Error<T>>::HttpFetchingError)?;
            let dot_price: DotPriceInfo = DotPriceInfo {
                name: v["data"]["name"].as_str().unwrap().as_bytes().to_vec(),
                usd: v["data"]["priceUsd"].as_str().unwrap().as_bytes().to_vec(),
            };

            Ok(dot_price)
        }
```
## 实现仅保留最新10个价格
```
        fn append_or_replace_price(price: (u64, Permill)) {
            // 仅保留最新10个Price
            Prices::<T>::mutate(|prices| {
                if prices.len() == NUM_VEC_LEN {
                    let _ = prices.pop_front();
                }
                prices.push_back(price);

                log::info!("...... Price vector: {:?}", prices);
            });
        }
```
## 将小数字符，拆分成整数部分和小数部分组成的元组
```
        fn convert_price_str(price_str: &str) -> Result<(u64, Permill), Error<T>> {
            let price_arr: Vec<&str> = price_str.split(".").collect();
            // log::info!("...... price_arr: {:?}", price_arr);

            let integer_str: &str = &price_arr[0];
            let decimal_str: &str = &price_arr[1][0..6];
            // log::info!("...... price_str split: {}.{} ", integer_str, decimal_str);

            let integer_parts: u64 = integer_str
                .parse::<u64>()
                .map_err(|_| <Error<T>>::ConvertPriceErr)?;
            let decimal_parts: u32 = decimal_str
                .parse::<u32>()
                .map_err(|_| <Error<T>>::ConvertPriceErr)?;

            let price_tuple: (u64, Permill) = (integer_parts, Permill::from_parts(decimal_parts));
            // log::info!("...... price_tuple: {:?} ", price_tuple);

            Ok(price_tuple)
        }
```
