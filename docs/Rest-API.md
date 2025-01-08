# Runes-DEx API

## Possible HTTP Status codes

- 200 - Ok - success.
- 400 - BadRequest - something is wrong with the data that was sent.
- 401 - Unauthorized - bad or lack of Authorization.
- 403 - Forbidden - no access to data, for example by policies.
- 404 - NotFound - no such route.
- 422 - UnprocessableEntity -.
- 500 - ServerError - error on the server that the client cannot fix.


## Generic error response

```json
{
    "code": "",
    "message": "",
    "reason": "" 
}
```

- `code` dublicates HTTP Status Code
   - Unauthorized
   - Forbidden
   - NotFound
   - Bad Request
   - Unprocessable Entity
   - Internal Server Error
- `message` internal message code. 
   - INTERNAL_ERROR
   - UNEXPECTED_RESPONSE
   - NOT_FOUND
   - INVALID_PAYLOAD
- `reason` is an optional error message.

## Routes

1. `GET /v1/version`

```json
{
    "app": "",
    "version": "",
    "build": ""
}
```

2. `GET /v1/runes?limit={limit}&cursor={cursor}&order={order}`

    - `cursor` is basically **RuneID**.
    - `order` - `asc` or `desc`.
    - `limit` is number of records per page: 1..500.

```json
{
    "next_cursor": "942942:23"
    "records": [{ 
        "id": "9321:123",
        "rune": "VISITLIONLIONXYZ",
        "display_name": "VISIT•LIONLION•XYZ",
        "symbol": "✌",
        "block": 2584337,
        "tx_id": 36,
        "mints": 0,
        "max_supply": 10000000,
        "minted": 0,
        "in_circulation": 0,
        "divisibility": 0,
        "turbo": false,
        "timestamp": 1711756336,
        "raw_data": ""
    }]
}
```

3. `GET /v1/runes/{rune}`

    - `rune` is string with Rune Unique Name. In example is equals to "UNCOMMONGOODS".

```json
{
  "id": "9321:123",
  "rune": "VISITLIONLIONXYZ",
  "display_name": "VISIT•LIONLION•XYZ",
  "symbol": "✌",
  "block": 2584337,
  "tx_id": 36,
  "mints": 0,
  "max_supply": 10000000,
  "minted": 0,
  "in_circulation": 0,
  "divisibility": 0,
  "turbo": false,
  "timestamp": 1711756336,
  "raw_data": ""
}
```

4. `GET /v1/pairs?limit={limit}&cursor={cursor}&order={order}`

    - `cursor` is `id` of pair.
    - `order` - `asc` or `desc`.
    - `limit` is number of records per page: 1..500.

```json
{
    "next_cursor": "23"
    "records": [
        {
            "id": "213",
            "base_asset": "UNCOMMONGOODS",
            "quote_asset" "BTC",
            "price": 11.666,
            "usd_price": 7864.33,
            "base_volume": 12321.333,
            "quote_volume": 12.323233
        }
    ]
}
```

4. `GET /v1/pairs/{base}-{quote}`

    - `base` is `rune` string with Rune Unique Name.
    - `quote` is an asset against witch rune will be traded. [ 'btc' ]

```json
{
    "id": "213",
    "base_asset": "UNCOMMONGOODS",
    "quote_asset" "BTC",
    "price": 11.666,
    "usd_price": 7864.33,
    "base_volume": 12321.333,
    "quote_volume": 12.323233
}
```

5. `GET /v1/pairs/{base}-{quote}/price-history?scale={scale}`

    - `base` is `rune` string with Rune Unique Name.
    - `quote` is an asset against witch rune will be traded. [ 'btc' ]
    - `scale` - [ '1h'| '1w' | '1m' ]

```json
{
    "records": [
        {
            "id": "123",
            "price": 11.666,
            "usd_price": 7864.33,
            "volume": 12321.333,
            "timestamp": 12321949412
        }
    ]
}
```
