# TopologyStateFilter

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**participant_ids** | Option<**Vec<String>**> | If this list is non-empty, only vetted packages hosted on participants listed in this field match the filter. Query the current Ledger API's participant's ID via the public ``GetParticipantId`` command in ``PartyManagementService``. | [optional]
**synchronizer_ids** | Option<**Vec<String>**> | If this list is non-empty, only vetted packages from the topology state of the synchronizers in this list match the filter. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


