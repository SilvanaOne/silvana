# GetPreferredPackagesResponse

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**package_references** | Option<[**Vec<models::PackageReference>**](PackageReference.md)> | The package references of the preferred packages. Must contain one package reference for each requested package-name.  If you build command submissions whose content depends on the returned preferred packages, then we recommend submitting the preferred package-ids in the ``package_id_selection_preference`` of the command submission to avoid race conditions with concurrent changes of the on-ledger package vetting state.  Required | [optional]
**synchronizer_id** | **String** | The synchronizer for which the package preferences are computed. If the synchronizer_id was specified in the request, then it matches the request synchronizer_id. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


