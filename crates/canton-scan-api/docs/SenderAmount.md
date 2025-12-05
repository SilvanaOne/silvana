# SenderAmount

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**party** | **String** | The sender who has transferred amulet.  | 
**input_amulet_amount** | Option<**String**> | Total amount of amulet input into this transfer, before deducting holding fees.  | [optional]
**input_app_reward_amount** | Option<**String**> | Total amount of app rewards input into this transfer.  | [optional]
**input_validator_reward_amount** | Option<**String**> | Total amount of validator rewards input into this transfer.  | [optional]
**input_sv_reward_amount** | Option<**String**> | Total amount of sv rewards input into this transfer.  | [optional]
**input_validator_faucet_amount** | Option<**String**> | Total amount of validator faucet coupon issuance input into this transfer.  | [optional]
**sender_change_fee** | **String** | Fee charged for returning change to the sender, which is the smaller of the left-over balance after paying for all outputs or one amulet create fee.  | 
**sender_change_amount** | **String** | The final amount of amulet returned to the sender after paying for all outputs and fees.  | 
**sender_fee** | **String** | Total fees paid by the sender, based on receiver's receiver_fee_ratio on outputs  | 
**holding_fees** | **String** | Holding fees paid by the sender on their input amulets.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


