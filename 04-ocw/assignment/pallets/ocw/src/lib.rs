#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use core::{convert::TryInto, fmt};
    use frame_support::pallet_prelude::*;
    use frame_system::{
        offchain::{
            AppCrypto, CreateSignedTransaction, SendSignedTransaction, SendUnsignedTransaction,
            SignedPayload, Signer, SigningTypes, SubmitTransaction,
        },
        pallet_prelude::*,
    };
    use parity_scale_codec::{Decode, Encode};
    use sp_arithmetic::per_things::Permill;
    use sp_core::crypto::KeyTypeId;
    use sp_runtime::{
        offchain as rt_offchain,
        offchain::{
            storage::StorageValueRef,
            storage_lock::{BlockAndTime, StorageLock},
        },
        traits::BlockNumberProvider,
        transaction_validity::{
            InvalidTransaction, TransactionSource, TransactionValidity, ValidTransaction,
        },
        RuntimeDebug,
    };
    use sp_std::{collections::vec_deque::VecDeque, prelude::*, str};

    use serde::{Deserialize, Deserializer};

    pub const KEY_TYPE: KeyTypeId = KeyTypeId(*b"demo");
    const NUM_VEC_LEN: usize = 10;
    const UNSIGNED_TXS_PRIORITY: u64 = 100;

    const HTTP_REMOTE_REQUEST_DOT_PRICE: &str = "https://api.coincap.io/v2/assets/polkadot";
    const HTTP_REMOTE_REQUEST: &str = "https://api.github.com/orgs/substrate-developer-hub";
    const HTTP_HEADER_USER_AGENT: &str = "jimmychu0807";

    const FETCH_TIMEOUT_PERIOD: u64 = 3000;
    const LOCK_TIMEOUT_EXPIRATION: u64 = FETCH_TIMEOUT_PERIOD + 1000;
    const LOCK_BLOCK_EXPIRATION: u32 = 3;

    pub mod crypto {
        use crate::KEY_TYPE;
        use sp_core::sr25519::Signature as Sr25519Signature;
        use sp_runtime::app_crypto::{app_crypto, sr25519};
        use sp_runtime::{traits::Verify, MultiSignature, MultiSigner};

        app_crypto!(sr25519, KEY_TYPE);

        pub struct TestAuthId;
        impl frame_system::offchain::AppCrypto<MultiSigner, MultiSignature> for TestAuthId {
            type RuntimeAppPublic = Public;
            type GenericSignature = sp_core::sr25519::Signature;
            type GenericPublic = sp_core::sr25519::Public;
        }

        impl
            frame_system::offchain::AppCrypto<
                <Sr25519Signature as Verify>::Signer,
                Sr25519Signature,
            > for TestAuthId
        {
            type RuntimeAppPublic = Public;
            type GenericSignature = sp_core::sr25519::Signature;
            type GenericPublic = sp_core::sr25519::Public;
        }
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
    pub struct Payload<Public> {
        number: u64,
        public: Public,
    }

    impl<T: SigningTypes> SignedPayload<T> for Payload<T::Public> {
        fn public(&self) -> T::Public {
            self.public.clone()
        }
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug)]
    pub struct PayloadPrice<Public> {
        price_tuple: (u64, Permill),
        public: Public,
    }

    impl<T: SigningTypes> SignedPayload<T> for PayloadPrice<T::Public> {
        fn public(&self) -> T::Public {
            self.public.clone()
        }
    }

    #[derive(Deserialize, Encode, Decode, Default)]
    struct GithubInfo {
        #[serde(deserialize_with = "de_string_to_bytes")]
        login: Vec<u8>,
        #[serde(deserialize_with = "de_string_to_bytes")]
        blog: Vec<u8>,
        public_repos: u32,
    }

    #[derive(Debug, Deserialize, Encode, Decode, Default)]
    struct IndexingData(Vec<u8>, u64);

    pub fn de_string_to_bytes<'de, D>(de: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(de)?;
        Ok(s.as_bytes().to_vec())
    }

    impl fmt::Debug for GithubInfo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                f,
                "{{ login: {}, blog: {}, public_repos: {} }}",
                str::from_utf8(&self.login).map_err(|_| fmt::Error)?,
                str::from_utf8(&self.blog).map_err(|_| fmt::Error)?,
                &self.public_repos
            )
        }
    }

    // Dot价格结构体 (方法1  Json字符串 自动转换成 需要 DotPrice, 和 DotPriceData)
    // #[derive(Deserialize, Encode, Decode, Default)]
    // struct DotPrice {
    //     data: DotPriceData,
    // }
    //
    // #[derive(Deserialize, Encode, Decode, Default)]
    // struct DotPriceData {
    //     #[serde(deserialize_with = "de_string_to_bytes")]
    //     name: Vec<u8>,
    //     #[serde(deserialize_with = "de_string_to_bytes", alias = "priceUsd")]
    //     price_usd: Vec<u8>,
    // }

    // Dot价格结构体 (方法2，手动拼接 DotPriceInfo)
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

    #[pallet::config]
    pub trait Config: frame_system::Config + CreateSignedTransaction<Call<Self>> {
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
        type Call: From<Call<Self>>;
        type AuthorityId: AppCrypto<Self::Public, Self::Signature>;
    }

    #[pallet::pallet]
    #[pallet::generate_store(pub(super) trait Store)]
    pub struct Pallet<T>(_);

    #[pallet::storage]
    #[pallet::getter(fn numbers)]
    pub type Numbers<T> = StorageValue<_, VecDeque<u64>, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn prices)]
    pub type Prices<T> = StorageValue<_, VecDeque<(u64, Permill)>, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        NewNumber(Option<T::AccountId>, u64),
        NewPrice(Option<T::AccountId>, (u64, Permill)),
    }

    #[pallet::error]
    pub enum Error<T> {
        UnknownOffchainMux,
        NoLocalAcctForSigning,
        OffchainSignedTxError,
        OffchainUnsignedTxError,
        OffchainUnsignedTxSignedPayloadError,
        HttpFetchingError,

        GetPriceErr,
        ConvertPriceErr,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn offchain_worker(block_number: T::BlockNumber) {
            const TX_TYPES: u32 = 5;
            let modu = block_number
                .try_into()
                .map_or(TX_TYPES, |bn: usize| (bn as u32) % TX_TYPES);

            log::info!(">>>>>> offchain workers! match mod:{}", modu);

            let result = match modu {
                1 => Self::offchain_signed_tx(block_number),
                2 => Self::offchain_unsigned_tx(block_number),
                3 => Self::offchain_unsigned_tx_signed_payload(block_number),
                4 => Self::fetch_github_info(),
                0 => Self::fetch_price_info(),
                _ => Err(Error::<T>::UnknownOffchainMux),
            };

            if let Err(e) = result {
                log::error!("...... offchain_worker error: {:?}", e);
            }
        }
    }

    #[pallet::validate_unsigned]
    impl<T: Config> ValidateUnsigned for Pallet<T> {
        type Call = Call<T>;

        fn validate_unsigned(_source: TransactionSource, call: &Self::Call) -> TransactionValidity {
            let valid_tx = |provide| {
                ValidTransaction::with_tag_prefix("ocw-demo")
                    .priority(UNSIGNED_TXS_PRIORITY)
                    .and_provides([&provide])
                    .longevity(3)
                    .propagate(true)
                    .build()
            };

            match call {
                Call::submit_number_unsigned(_number) => {
                    valid_tx(b"submit_number_unsigned".to_vec())
                }
                Call::submit_number_unsigned_with_signed_payload(ref payload, ref signature) => {
                    if !SignedPayload::<T>::verify::<T::AuthorityId>(payload, signature.clone()) {
                        return InvalidTransaction::BadProof.into();
                    }
                    valid_tx(b"submit_number_unsigned_with_signed_payload".to_vec())
                }
                // Call::submit_price_unsigned(_number) => valid_tx(b"submit_price_unsigned".to_vec()),
                Call::submit_price_unsigned_with_signed_payload(
                    ref payloadprice,
                    ref signature,
                ) => {
                    if !SignedPayload::<T>::verify::<T::AuthorityId>(
                        payloadprice,
                        signature.clone(),
                    ) {
                        return InvalidTransaction::BadProof.into();
                    }
                    valid_tx(b"submit_price_unsigned_with_signed_payload".to_vec())
                }
                _ => InvalidTransaction::Call.into(),
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::weight(10000)]
        pub fn submit_number_signed(origin: OriginFor<T>, number: u64) -> DispatchResult {
            let who = ensure_signed(origin)?;
            log::info!("...... submit_number_signed: ({}, {:?})", number, who);
            Self::append_or_replace_number(number);
            Self::deposit_event(Event::NewNumber(Some(who), number));
            Ok(())
        }

        #[pallet::weight(10000)]
        pub fn submit_number_unsigned(origin: OriginFor<T>, number: u64) -> DispatchResult {
            let _ = ensure_none(origin)?;
            log::info!("......  submit_number_unsigned: {}", number);
            Self::append_or_replace_number(number);
            Self::deposit_event(Event::NewNumber(None, number));
            Ok(())
        }

        #[pallet::weight(10000)]
        pub fn submit_number_unsigned_with_signed_payload(
            origin: OriginFor<T>,
            payload: Payload<T::Public>,
            _signature: T::Signature,
        ) -> DispatchResult {
            let _ = ensure_none(origin)?;
            let Payload { number, public } = payload;
            log::info!(
                "...... submit_number_unsigned_with_signed_payload: ({}, {:?})",
                number,
                public
            );
            Self::append_or_replace_number(number);
            Self::deposit_event(Event::NewNumber(None, number));
            Ok(())
        }

        // #[pallet::weight(10000)]
        // pub fn submit_price_unsigned(
        //     origin: OriginFor<T>,
        //     price: (u64, Permill),
        // ) -> DispatchResult {
        //     let _ = ensure_none(origin)?;
        //     log::info!("...... submit_price_unsigned: {:?}", price);
        //     Self::append_or_replace_price(price);
        //     Self::deposit_event(Event::NewPrice(None, price));
        //     Ok(())
        // }
        
        #[pallet::weight(10000)]
        pub fn submit_price_unsigned_with_signed_payload(
            origin: OriginFor<T>,
            payloadprice: PayloadPrice<T::Public>,
            _signature: T::Signature,
        ) -> DispatchResult {
            let _ = ensure_none(origin)?;
            let PayloadPrice {
                price_tuple,
                public,
            } = payloadprice;
            log::info!(
                "...... submit_price_unsigned_with_signed_payload: ({:?}, {:?})",
                price_tuple,
                public
            );
            Self::append_or_replace_price(price_tuple);
            Self::deposit_event(Event::NewPrice(None, price_tuple));
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
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

        // 将小数字符，拆分成整数部分和小数部分组成的元组
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

        fn fetch_price_info() -> Result<(), Error<T>> {
            // TODO: 这是你们的功课
            // 利用 offchain worker 取出 DOT 当前对 USD 的价格，并把写到一个 Vec 的存储里，
            // 你们自己选一种方法提交回链上，并在代码注释为什么用这种方法提交回链上最好。只保留当前最近的 10 个价格，
            // 其他价格可丢弃 （就是 Vec 的长度长到 10 后，这时再插入一个值时，要先丢弃最早的那个值）。
            // 取得的价格 parse 完后，放在以下存儲：
            // pub type Prices<T> = StorageValue<_, VecDeque<(u64, Permill)>, ValueQuery>
            // 这个 http 请求可得到当前 DOT 价格：
            // [https://api.coincap.io/v2/assets/polkadot](https://api.coincap.io/v2/assets/polkadot)。

            log::info!("...... fetch_price_info");

            let dot_price =
                Self::fetch_n_parse_to_dot_price().map_err(|_| <Error<T>>::GetPriceErr)?;

            // 获取 dot usd 对应的字符串
            let usd_str: &str = str::from_utf8(&dot_price.usd).unwrap();

            //折分成整部分和小数部分的元组
            let price_tuple: (u64, Permill) =
                Self::convert_price_str(usd_str).map_err(|_| <Error<T>>::ConvertPriceErr)?;
            log::info!("...... price_tuple: {:?}", price_tuple);

            // 使用不签名方式，提交到链上。
            // let call = Call::submit_price_unsigned(price_tuple);
            // SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(call.into()).map_err(
            //     |_| {
            //         log::error!("...... Failed in offchain_unsigned_tx");
            //         <Error<T>>::OffchainUnsignedTxError
            //     },
            // )

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

            
            // 记录到 ocw 链下的独立存储
            // let s_price = StorageValueRef::persistent(b"offchain-demo::dot-price");
            // let mut lock = StorageLock::<BlockAndTime<Self>>::with_block_and_time_deadline(
            //     b"offchain-demo::lock",
            //     LOCK_BLOCK_EXPIRATION,
            //     rt_offchain::Duration::from_millis(LOCK_TIMEOUT_EXPIRATION),
            // );
            // if let Ok(_guard) = lock.try_lock() {
            //     match Self::fetch_n_parse_to_dot_price() {
            //         Ok(dot_price) => {
            //             log::info!("......dot_price: {:?}", dot_price);
            //             //获取 dot usd 对应的字符串
            //             let usd_str: &str = str::from_utf8(&dot_price.usd).unwrap();
            //             //折分成整部分和小数部分的元组
            //             let price_tuple: (u64, Permill) = Self::convert_price_str(usd_str).map_err(|_| <Error<T>>::ConvertPriceErr)?;
            //             log::info!("......{:?}", price_tuple);
            //             // 写入链下的独立存储
            //             s_price.set(&dot_price);
            //             // 读取链下的独立存储
            //             if let Ok(Some(s_dotprice)) = s_price.get::<DotPriceInfo>() {
            //                 log::info!("......cached s_dotprice: {:?}", s_dotprice);
            //             }
            //         }
            //         Err(err) => { return Err(err);}
            //     }
            // }
            // Ok()
        }

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

            // 方法1： Json字符串 自动转换成 dot_price
            // let s_data: DotPrice =
            //     serde_json::from_str(&resp_str).map_err(|_| <Error<T>>::HttpFetchingError)?;
            // log::info!("...... s_data.data.name: {}",str::from_utf8(&s_data.data.name).unwrap());
            // log::info!("...... s_data.data.price_usd: {:?}",str::from_utf8(&s_data.data.price_usd).unwrap());

            // 方法2： 转换为serde_json 再转换成 dot_price
            let v: serde_json::Value =
                serde_json::from_str(&resp_str).map_err(|_| <Error<T>>::HttpFetchingError)?;
            let dot_price: DotPriceInfo = DotPriceInfo {
                name: v["data"]["name"].as_str().unwrap().as_bytes().to_vec(),
                usd: v["data"]["priceUsd"].as_str().unwrap().as_bytes().to_vec(),
            };

            Ok(dot_price)
        }

        fn append_or_replace_number(number: u64) {
            Numbers::<T>::mutate(|numbers| {
                if numbers.len() == NUM_VEC_LEN {
                    let _ = numbers.pop_front();
                }
                numbers.push_back(number);

                log::info!("...... Number vector: {:?}", numbers);
            });
        }

        fn fetch_github_info() -> Result<(), Error<T>> {
            log::info!("...... fetch_github_info! ");

            let s_info = StorageValueRef::persistent(b"offchain-demo::gh-info");
            if let Ok(Some(gh_info)) = s_info.get::<GithubInfo>() {
                log::info!("...... cached gh-info: {:?}", gh_info);
                return Ok(());
            }

            let mut lock = StorageLock::<BlockAndTime<Self>>::with_block_and_time_deadline(
                b"offchain-demo::lock",
                LOCK_BLOCK_EXPIRATION,
                rt_offchain::Duration::from_millis(LOCK_TIMEOUT_EXPIRATION),
            );

            if let Ok(_guard) = lock.try_lock() {
                match Self::fetch_n_parse() {
                    Ok(gh_info) => {
                        s_info.set(&gh_info);
                    }
                    Err(err) => {
                        return Err(err);
                    }
                }
            }
            Ok(())
        }

        fn fetch_n_parse() -> Result<GithubInfo, Error<T>> {
            let resp_bytes = Self::fetch_from_remote(HTTP_REMOTE_REQUEST).map_err(|e| {
                log::error!("...... fetch_from_remote error: {:?}", e);
                <Error<T>>::HttpFetchingError
            })?;

            let resp_str =
                str::from_utf8(&resp_bytes).map_err(|_| <Error<T>>::HttpFetchingError)?;
            log::info!("{}", resp_str);

            let gh_info: GithubInfo =
                serde_json::from_str(&resp_str).map_err(|_| <Error<T>>::HttpFetchingError)?;
            Ok(gh_info)
        }

        fn fetch_from_remote(remote_request: &str) -> Result<Vec<u8>, Error<T>> {
            log::info!("...... sending request to: {}", remote_request);

            let request = rt_offchain::http::Request::get(remote_request);

            let timeout = sp_io::offchain::timestamp()
                .add(rt_offchain::Duration::from_millis(FETCH_TIMEOUT_PERIOD));

            let pending = request
                .add_header("User-Agent", HTTP_HEADER_USER_AGENT)
                .deadline(timeout) // Setting the timeout time
                .send() // Sending the request out by the host
                .map_err(|_| <Error<T>>::HttpFetchingError)?;

            let response = pending
                .try_wait(timeout)
                .map_err(|_| <Error<T>>::HttpFetchingError)?
                .map_err(|_| <Error<T>>::HttpFetchingError)?;

            if response.code != 200 {
                log::error!(
                    "...... Unexpected http request status code: {}",
                    response.code
                );
                return Err(<Error<T>>::HttpFetchingError);
            }

            Ok(response.body().collect::<Vec<u8>>())
        }

        fn offchain_signed_tx(block_number: T::BlockNumber) -> Result<(), Error<T>> {
            log::info!(
                "...... offchain_signed_tx! block_number : {:?}",
                block_number
            );
            let signer = Signer::<T, T::AuthorityId>::any_account();
            let number: u64 = block_number.try_into().unwrap_or(0);
            let result = signer.send_signed_transaction(|_acct| Call::submit_number_signed(number));
            if let Some((acc, res)) = result {
                if res.is_err() {
                    log::error!("...... failure: offchain_signed_tx: tx sent: {:?}", acc.id);
                    return Err(<Error<T>>::OffchainSignedTxError);
                }
                return Ok(());
            }
            log::error!("...... No local account available");
            Err(<Error<T>>::NoLocalAcctForSigning)
        }

        fn offchain_unsigned_tx(block_number: T::BlockNumber) -> Result<(), Error<T>> {
            log::info!(
                "...... offchain_unsigned_tx! block_number : {:?}",
                block_number
            );
            let number: u64 = block_number.try_into().unwrap_or(0);
            let call = Call::submit_number_unsigned(number);
            SubmitTransaction::<T, Call<T>>::submit_unsigned_transaction(call.into()).map_err(
                |_| {
                    log::error!("...... Failed in offchain_unsigned_tx");
                    <Error<T>>::OffchainUnsignedTxError
                },
            )
        }

        fn offchain_unsigned_tx_signed_payload(
            block_number: T::BlockNumber,
        ) -> Result<(), Error<T>> {
            log::info!(
                "...... offchain_unsigned_tx_signed_payload! block_number : {:?}",
                block_number
            );
            let signer = Signer::<T, T::AuthorityId>::any_account();
            let number: u64 = block_number.try_into().unwrap_or(0);
            let result = signer.send_unsigned_transaction(
                |acct| Payload {
                    number,
                    public: acct.public.clone(),
                },
                Call::submit_number_unsigned_with_signed_payload,
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
    }

    impl<T: Config> BlockNumberProvider for Pallet<T> {
        type BlockNumber = T::BlockNumber;

        fn current_block_number() -> Self::BlockNumber {
            <frame_system::Pallet<T>>::block_number()
        }
    }
}
