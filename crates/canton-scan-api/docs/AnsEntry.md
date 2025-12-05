# AnsEntry

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**contract_id** | Option<**String**> | If present, Daml contract ID of template `Splice.Ans:AnsEntry`. If absent, this is a DSO-provided entry for either the DSO or an SV.  | [optional]
**user** | **String** | Owner party ID of this ANS entry. | 
**name** | **String** | The ANS entry name. | 
**url** | **String** | Either empty, or an http/https URL supplied by the `user`. | 
**description** | **String** | Arbitrary description text supplied by `user`; may be empty. | 
**expires_at** | Option<**String**> | Time after which this ANS entry expires; if renewed, it will have a new `contract_id` and `expires_at`. If `null` or absent, does not expire; this is the case only for special entries provided by the DSO.  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


