# ValidatorReceivedFaucets

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**validator** | **String** | The party ID of the onboarded validator | 
**num_rounds_collected** | **i64** | how many rounds the validator has received a faucet for; guaranteed that collected + missed = last - first + 1  | 
**num_rounds_missed** | **i64** | how many rounds between firstCollected and lastCollected in which the validator failed to collect (i.e. was not active or available); can at most be max(0, lastCollected - firstCollected - 1).  | 
**first_collected_in_round** | **i64** | the round number when this validator started collecting faucets; the validator definitely recorded liveness in this round  | 
**last_collected_in_round** | **i64** | The most recent round number in which the validator collected a faucet; the validator definitely recorded liveness in this round.  Will equal `firstCollected` if the validator has collected in only one round  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


