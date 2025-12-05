# Instrument

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **String** | The unique identifier assigned by the admin to the instrument. | 
**name** | **String** | The display name for the instrument recommended by the instrument admin. This is not necessarily unique. | 
**symbol** | **String** | The symbol for the instrument recommended by the instrument admin. This is not necessarily unique. | 
**total_supply** | Option<**String**> | Decimal encoded current total supply of the instrument. | [optional]
**total_supply_as_of** | Option<**String**> | The timestamp when the total supply was last computed. | [optional]
**decimals** | **i32** | The number of decimal places used by the instrument.  Must be a number between 0 and 10, as the Daml interfaces represent holding amounts as `Decimal` values, which use 10 decimal places and are precise for 38 digits. Setting this to 0 means that the instrument can only be held in whole units.  This number SHOULD be used for display purposes in a wallet to decide how many decimal places to show and accept when displaying or entering amounts.  | [default to 10]
**supported_apis** | **std::collections::HashMap<String, i32>** | Map from token standard API name to the minor version of the API supported, e.g., splice-api-token-metadata-v1 -> 1 where the `1` corresponds to the minor version.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


