# HoldingsSummary

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**party_id** | **String** | Owner party ID of the amulet. Guaranteed to be unique among `summaries`.  | 
**total_unlocked_coin** | **String** | Sum of unlocked amulet at time of reception, not counting holding fees deducted since.  | 
**total_locked_coin** | **String** | Sum of locked amulet at time of original amulet reception, not counting holding fees deducted since.  | 
**total_coin_holdings** | **String** | `total_unlocked_coin` + `total_locked_coin`.  | 
**accumulated_holding_fees_unlocked** | **String** | Sum of holding fees as of `computed_as_of_round` that apply to unlocked amulet.  | 
**accumulated_holding_fees_locked** | **String** | Sum of holding fees as of `computed_as_of_round` that apply to locked amulet, including fees applied since the amulet's creation round.  | 
**accumulated_holding_fees_total** | **String** | Same as `accumulated_holding_fees_unlocked` + `accumulated_holding_fees_locked`.  | 
**total_available_coin** | **String** | Same as `total_unlocked_coin` - `accumulated_holding_fees_unlocked`. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


